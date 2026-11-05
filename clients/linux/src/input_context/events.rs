//! GTK press/repeat/release and IM commit routing; only emitted presses own releases.
use super::*;
use crate::input::Action;
use crate::keyboard;

pub fn install(workspace: &Rc<RefCell<Workspace>>) {
    *workspace.borrow().input.workspace.borrow_mut() = Rc::downgrade(workspace);
    let area = workspace.borrow().area.clone();
    let weak = Rc::downgrade(workspace);
    area.connect_has_focus_notify(move |area| {
        if let Some(work) = weak.upgrade() {
            let input = work.borrow().input.clone();
            if !area.has_focus() {
                work.borrow_mut().cancel_input();
            }
            input.set_focused(area.has_focus());
            work.borrow_mut().update_terminal_focus();
        }
    });
    let weak = Rc::downgrade(workspace);
    area.connect_unrealize(move |_| {
        if let Some(work) = weak.upgrade() {
            work.borrow_mut().cancel_input();
            let input = work.borrow().input.clone();
            input.set_focused(false);
            input.set_target(None);
            input.context.set_client_widget(None::<&gtk::Widget>);
        }
    });
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(workspace);
    keys.connect_key_pressed(move |controller, key, code, modifiers| {
        let Some(work) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let input = work.borrow().input.clone();
        if let Some(action) = input::action(key, modifiers) {
            match action {
                Action::Paste => input.paste(),
                Action::CopyScreen => {
                    if let Some(text) = work.borrow().copy_screen() {
                        input.copy_screen(&text);
                    }
                }
                action => work.borrow_mut().action(action),
            }
            return glib::Propagation::Stop;
        }
        if !input.preedit.is_visible() && work.borrow_mut().scroll_key(key, modifiers) {
            input.cancel_preedit();
            return glib::Propagation::Stop;
        }
        if input.target.get().is_none() {
            return glib::Propagation::Stop;
        }
        let event = input.keyboard.borrow_mut().press(code);
        let modifiers = keyboard::modifier_state(key, modifiers, true, false);
        let mut stroke = keyboard::gtk::stroke(controller, key, code, modifiers);
        if input::encode(key, modifiers, false).is_some() {
            work.borrow_mut().prepare_input();
        }
        let was_preedit = input.preedit.is_visible();
        let before_preedit = input.preedits.get();
        let before_commit = input.commits.get();
        input.filtered_text.borrow_mut().clear();
        input.filtering.set(true);
        // Filtering may synchronously emit commit/preedit; never hold a Workspace borrow.
        let handled = input.filter(controller);
        input.filtering.set(false);
        let committed = input.commits.get() != before_commit;
        let text = std::mem::take(&mut *input.filtered_text.borrow_mut());
        let direct_text = !was_preedit
            && input.preedits.get() == before_preedit
            && key.to_unicode().is_some_and(|c| text == c.to_string());
        if committed && !direct_text {
            input.commit_text(&text);
        } else if !handled || direct_text {
            if committed {
                stroke.text = Some(text);
            } else if !modifiers.intersects(
                gdk::ModifierType::CONTROL_MASK
                    | gdk::ModifierType::ALT_MASK
                    | gdk::ModifierType::SUPER_MASK
                    | gdk::ModifierType::META_MASK,
            ) {
                stroke.text = key
                    .to_unicode()
                    .filter(|c| !c.is_control())
                    .map(|c| c.to_string());
            }
            if let Some(bytes) = keyboard::encode(&stroke, event, input.modes.get()) {
                if let Some(pane_id) = input.target.get() {
                    input.send(Message::Keys { pane_id, bytes });
                    input
                        .keyboard
                        .borrow_mut()
                        .sent(code, stroke, input.modes.get());
                }
            }
        }
        // Modifier events also update link cursor state without requiring pointer motion.
        work.borrow_mut().refresh_hover(modifiers);
        glib::Propagation::Stop
    });
    let weak = Rc::downgrade(workspace);
    keys.connect_key_released(move |controller, key, code, modifiers| {
        let Some(work) = weak.upgrade() else {
            return;
        };
        let input = work.borrow().input.clone();
        if !input.keyboard.borrow().held(code) {
            return;
        }
        let other = input.keyboard.borrow().other_modifier(key);
        let modifiers = keyboard::modifier_state(key, modifiers, false, other);
        let bytes = input
            .keyboard
            .borrow_mut()
            .release(code, modifiers, input.modes.get());
        if let Some(bytes) = bytes {
            if let Some(pane_id) = input.target.get() {
                // Releases never move a history viewport back to live.
                work.borrow().send_key_release(pane_id, bytes);
            }
        } else {
            input.filter(controller);
        }
        work.borrow_mut().refresh_hover(modifiers);
    });
    area.add_controller(keys);
}
