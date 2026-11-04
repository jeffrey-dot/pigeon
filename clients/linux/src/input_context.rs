//! GTK input-method and clipboard lifecycle, independent of the terminal renderer.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gdk, glib};
use gtk4 as gtk;
use pigeon_core::api::PaneId;

use crate::host::HostLink;
use crate::input::{self, Action};
use crate::view::Workspace;

pub struct InputContext {
    context: gtk::IMMulticontext,
    preedit: gtk::Label,
    area: gtk::GLArea,
    host: HostLink,
    target: Cell<Option<PaneId>>,
    focused: Cell<bool>,
    generation: Cell<u64>,
    commits: Cell<u64>,
    cursor: Cell<[i32; 4]>,
}

impl InputContext {
    pub fn new(area: &gtk::GLArea, root: &gtk::Overlay, host: HostLink) -> Rc<Self> {
        let context = gtk::IMMulticontext::new();
        context.set_client_widget(Some(area));
        context.set_use_preedit(true);
        context.set_input_purpose(gtk::InputPurpose::Terminal);
        let preedit = gtk::Label::new(None);
        preedit.set_halign(gtk::Align::Start);
        preedit.set_valign(gtk::Align::Start);
        preedit.set_can_target(false);
        preedit.set_single_line_mode(true);
        preedit.set_ellipsize(gtk::pango::EllipsizeMode::End);
        preedit.set_visible(false);
        // Attributes style plain text; pane text never enters a markup parser.
        let attrs = gtk::pango::AttrList::new();
        attrs.insert(gtk::pango::AttrColor::new_background(
            0x2222, 0x2222, 0x2222,
        ));
        attrs.insert(gtk::pango::AttrColor::new_foreground(
            0xffff, 0xffff, 0xffff,
        ));
        attrs.insert(gtk::pango::AttrInt::new_underline(
            gtk::pango::Underline::Single,
        ));
        attrs.insert(gtk::pango::AttrString::new_family("monospace"));
        preedit.set_attributes(Some(&attrs));
        root.add_overlay(&preedit);
        root.set_clip_overlay(&preedit, true);
        let input = Rc::new(Self {
            context,
            preedit,
            area: area.clone(),
            host,
            target: Cell::new(None),
            focused: Cell::new(false),
            generation: Cell::new(0),
            commits: Cell::new(0),
            cursor: Cell::new([0, 0, 1, 1]),
        });
        let weak = Rc::downgrade(&input);
        input.context.connect_commit(move |_, text| {
            if let Some(input) = weak.upgrade() {
                input.commits.set(input.commits.get().wrapping_add(1));
                input.preedit.set_visible(false);
                if let Some(pane) = input.target.get().filter(|_| input.focused.get()) {
                    if let Some(message) = input::commit(pane, text) {
                        input.host.send(message);
                    }
                }
            }
        });
        let weak = Rc::downgrade(&input);
        input.context.connect_preedit_changed(move |context| {
            if let Some(input) = weak.upgrade() {
                let (text, _, caret) = context.preedit_string();
                let offset = text
                    .char_indices()
                    .nth(caret.max(0) as usize)
                    .map_or(text.len(), |(offset, _)| offset);
                input
                    .preedit
                    .set_text(&format!("{}│{}", &text[..offset], &text[offset..]));
                input.preedit.set_visible(
                    !text.is_empty() && input.focused.get() && input.target.get().is_some(),
                );
                input.position_preedit();
            }
        });
        input
    }

    pub fn set_target(&self, pane: Option<PaneId>) {
        if self.target.get() == pane {
            return;
        }
        // Some IM modules commit during reset: discard that text before changing panes.
        self.target.set(None);
        self.invalidate();
        self.context.reset();
        self.preedit.set_visible(false);
        self.target.set(pane);
    }

    fn set_focused(&self, focused: bool) {
        if self.focused.replace(focused) == focused {
            return;
        }
        if focused {
            self.context.focus_in();
        } else {
            self.invalidate();
            self.context.reset();
            self.context.focus_out();
            self.preedit.set_visible(false);
        }
    }

    fn invalidate(&self) {
        self.generation.set(self.generation.get().wrapping_add(1));
    }

    pub fn set_cursor(&self, rect: [i32; 4]) {
        if self.cursor.replace(rect) == rect {
            return;
        }
        self.context
            .set_cursor_location(&gdk::Rectangle::new(rect[0], rect[1], rect[2], rect[3]));
        self.position_preedit();
    }

    fn position_preedit(&self) {
        let [x, y, _, h] = self.cursor.get();
        // Widget measurement includes our existing margins, feeding position back into size.
        let (label_width, label_height) = self.preedit.layout().pixel_size();
        let width = self.area.width().max(1);
        let height = self.area.height().max(1);
        self.preedit
            .set_margin_start(x.clamp(0, (width - label_width).max(0)));
        self.preedit
            .set_margin_top(y.clamp(0, (height - label_height.max(h)).max(0)));
    }

    pub fn paste(self: &Rc<Self>) {
        let Some(pane) = self.target.get().filter(|_| self.focused.get()) else {
            return;
        };
        let generation = self.generation.get();
        let weak = Rc::downgrade(self);
        self.area
            .clipboard()
            .read_text_async(None::<&gtk::gio::Cancellable>, move |result| {
                let Some(input) = weak.upgrade() else {
                    return;
                };
                if input.generation.get() != generation || input.target.get() != Some(pane) {
                    return;
                }
                match result {
                    Ok(Some(text)) => {
                        if let Some(message) = input::paste(pane, &text) {
                            input.host.send(message);
                        }
                    }
                    Ok(None) => input.area.error_bell(),
                    Err(error) => {
                        eprintln!("pigeon: clipboard read failed: {error}");
                        input.area.error_bell();
                    }
                }
            });
    }

    pub fn copy_screen(&self, text: &str) {
        // GDK publishes a local content provider; transfer to another application is async.
        self.area.clipboard().set_text(text);
    }

    fn filter(&self, controller: &gtk::EventControllerKey) -> bool {
        controller
            .current_event()
            .is_some_and(|event| self.context.filter_keypress(event))
    }
}

pub fn install(workspace: &Rc<RefCell<Workspace>>) {
    let area = workspace.borrow().area.clone();
    area.set_tooltip_text(Some(
        "Ctrl+Shift+C: Copy current pane screen\nCtrl+Shift+V: Paste",
    ));
    let weak = Rc::downgrade(&workspace.borrow().input);
    area.connect_has_focus_notify(move |area| {
        if let Some(input) = weak.upgrade() {
            input.set_focused(area.has_focus());
        }
    });
    let weak = Rc::downgrade(&workspace.borrow().input);
    area.connect_unrealize(move |_| {
        if let Some(input) = weak.upgrade() {
            input.set_focused(false);
            input.set_target(None);
            input.context.set_client_widget(None::<&gtk::Widget>);
        }
    });
    let keys = gtk::EventControllerKey::new();
    let weak = Rc::downgrade(workspace);
    keys.connect_key_pressed(move |controller, key, _, modifiers| {
        let Some(workspace) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        let input = workspace.borrow().input.clone();
        if let Some(action) = input::action(key, modifiers) {
            match action {
                Action::Paste => input.paste(),
                Action::CopyScreen => {
                    if let Some(text) = workspace.borrow().copy_screen() {
                        input.copy_screen(&text);
                    }
                }
                action => workspace.borrow_mut().action(action),
            }
            return glib::Propagation::Stop;
        }
        // filter_keypress may synchronously emit commit/preedit: no Workspace borrow here.
        let before = input.commits.get();
        let handled = input.filter(controller);
        let committed = input.commits.get() != before;
        let result = workspace.borrow().key(key, modifiers, handled, committed);
        result
    });
    let weak = Rc::downgrade(&workspace.borrow().input);
    keys.connect_key_released(move |controller, key, _, modifiers| {
        if input::action(key, modifiers).is_none() {
            if let Some(input) = weak.upgrade() {
                input.filter(controller);
            }
        }
    });
    area.add_controller(keys);
}
