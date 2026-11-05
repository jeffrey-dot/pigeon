//! A compact, on-demand search panel inside the Harp terminal card.
use crate::view::Workspace;
use gtk4::{self as gtk, glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub fn install(workspace: &Rc<RefCell<Workspace>>, window: &gtk::ApplicationWindow) -> gtk::Button {
    let panel = gtk::Box::new(gtk::Orientation::Vertical, 6);
    panel.add_css_class("pigeon-records");
    panel.set_halign(gtk::Align::End);
    panel.set_valign(gtk::Align::Start);
    panel.set_margin_top(10);
    panel.set_margin_end(10);
    panel.set_visible(false);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let field = gtk::SearchEntry::new();
    field.set_placeholder_text(Some("Find in pane history"));
    field.set_width_chars(24);
    let previous = gtk::Button::with_label("↑");
    previous.set_tooltip_text(Some("Previous match"));
    let next = gtk::Button::with_label("↓");
    next.set_tooltip_text(Some("Next match"));
    let close = gtk::Button::with_label("Close");
    top.append(&field);
    top.append(&previous);
    top.append(&next);
    top.append(&close);
    let bottom = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let sensitive = gtk::CheckButton::with_label("Match ASCII Case");
    sensitive.set_active(true);
    let stop = gtk::Button::with_label("Stop");
    let again = gtk::Button::with_label("Search Again");
    bottom.append(&sensitive);
    bottom.append(&stop);
    bottom.append(&again);
    let summary = gtk::Label::new(None);
    summary.set_xalign(0.0);
    summary.add_css_class("pigeon-quiet");
    panel.append(&top);
    panel.append(&bottom);
    panel.append(&summary);
    workspace.borrow().root.add_overlay(&panel);
    let edit = Rc::new(Cell::new(0u64));
    let restart: Rc<dyn Fn()> = {
        let weak = Rc::downgrade(workspace);
        let field = field.clone();
        let sensitive = sensitive.clone();
        let edit = edit.clone();
        Rc::new(move || {
            let token = edit.get().wrapping_add(1);
            edit.set(token);
            let context = weak.upgrade().and_then(|work| {
                work.borrow()
                    .search
                    .as_ref()
                    .map(|search| (search.pane, search.selection))
            });
            if let Some(work) = weak.upgrade() {
                if let Some(search) = work.borrow_mut().search.as_mut() {
                    search.cancel();
                }
            }
            let weak = weak.clone();
            let field = field.clone();
            let sensitive = sensitive.clone();
            let edit = edit.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(150), move || {
                if edit.get() != token {
                    return;
                }
                if let Some(work) = weak.upgrade() {
                    if work.borrow().search.as_ref().map(|s| (s.pane, s.selection)) != context {
                        return;
                    }
                    work.borrow_mut()
                        .begin_search(field.text().to_string(), sensitive.is_active());
                }
            });
        })
    };
    let action = restart.clone();
    field.connect_search_changed(move |_| action());
    let action = restart.clone();
    sensitive.connect_toggled(move |_| action());
    let action = restart.clone();
    again.connect_clicked(move |_| action());
    for (button, back) in [(&previous, true), (&next, false)] {
        let weak = Rc::downgrade(workspace);
        button.connect_clicked(move |_| {
            if let Some(work) = weak.upgrade() {
                work.borrow_mut().navigate_search(back);
            }
        });
    }
    let weak = Rc::downgrade(workspace);
    field.connect_activate(move |_| {
        if let Some(work) = weak.upgrade() {
            work.borrow_mut().navigate_search(false);
        }
    });
    let weak = Rc::downgrade(workspace);
    let token = edit.clone();
    stop.connect_clicked(move |_| {
        token.set(token.get().wrapping_add(1));
        if let Some(work) = weak.upgrade() {
            if let Some(search) = work.borrow_mut().search.as_mut() {
                search.cancel();
            }
        }
    });
    let hide: Rc<dyn Fn()> = {
        let weak = Rc::downgrade(workspace);
        let panel = panel.clone();
        let edit = edit.clone();
        Rc::new(move || {
            edit.set(edit.get().wrapping_add(1));
            if let Some(work) = weak.upgrade() {
                work.borrow_mut().close_search(true);
                panel.set_visible(false);
                let area = work.borrow().area.clone();
                area.grab_focus();
            }
        })
    };
    let action = hide.clone();
    close.connect_clicked(move |_| action());
    let action = hide.clone();
    field.connect_stop_search(move |_| action());
    let keys = gtk::EventControllerKey::new();
    keys.set_propagation_phase(gtk::PropagationPhase::Capture);
    let weak = Rc::downgrade(workspace);
    let shown = panel.clone();
    let entry = field.clone();
    let show: Rc<dyn Fn()> = Rc::new(move || {
        if let Some(work) = weak.upgrade() {
            if work.borrow_mut().open_search() {
                entry.set_sensitive(work.borrow().search_supported);
                shown.set_visible(true);
                entry.grab_focus();
                entry.select_region(0, -1);
                if !entry.text().is_empty() && work.borrow().search_supported {
                    restart();
                }
            }
        }
    });
    let action = show.clone();
    let shown = panel.clone();
    let weak = Rc::downgrade(workspace);
    keys.connect_key_pressed(move |_, key, _, modifiers| {
        if key == gtk::gdk::Key::Escape && shown.is_visible() {
            hide();
            return glib::Propagation::Stop;
        }
        if shown.is_visible()
            && key == gtk::gdk::Key::Return
            && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)
        {
            if let Some(work) = weak.upgrade() {
                work.borrow_mut().navigate_search(true);
            }
            return glib::Propagation::Stop;
        }
        if key.to_lower() != gtk::gdk::Key::f
            || !modifiers
                .contains(gtk::gdk::ModifierType::CONTROL_MASK | gtk::gdk::ModifierType::SHIFT_MASK)
            || modifiers
                .intersects(gtk::gdk::ModifierType::ALT_MASK | gtk::gdk::ModifierType::SUPER_MASK)
        {
            return glib::Propagation::Proceed;
        }
        action();
        glib::Propagation::Stop
    });
    window.add_controller(keys);
    let weak = Rc::downgrade(workspace);
    glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
        let Some(work) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mut work = work.borrow_mut();
        if let Some(search) = work.search.as_ref() {
            summary.set_text(&search.summary());
            stop.set_sensitive(search.cancellable());
            previous.set_sensitive(!search.hits.is_empty());
            next.set_sensitive(!search.hits.is_empty());
            work.search_page();
        } else if panel.is_visible() {
            edit.set(edit.get().wrapping_add(1));
            panel.set_visible(false);
        }
        glib::ControlFlow::Continue
    });
    let button = gtk::Button::from_icon_name("edit-find-symbolic");
    button.add_css_class("pigeon-icon");
    button.set_tooltip_text(Some("Find in pane · Ctrl+Shift+F"));
    button.connect_clicked(move |_| show());
    button
}
