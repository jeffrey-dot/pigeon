//! GTK native local session list. Project discovery is client-side filesystem I/O.
use super::{grouping, records::RecordsView, rows, widgets};
use crate::view::Workspace;
use gtk4::{self as gtk, prelude::*};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::{Rc, Weak};

pub const WIDTH: i32 = 272;

pub struct SidebarView {
    pub root: gtk::Box,
    list: gtk::ListBox,
    connection: gtk::Label,
    records: Rc<RecordsView>,
    work: Weak<RefCell<Workspace>>,
    projects: RefCell<HashMap<String, (String, String)>>,
    signature: RefCell<String>,
}

impl SidebarView {
    pub fn new(workspace: &Rc<RefCell<Workspace>>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 0);
        root.add_css_class("pigeon-sidebar");
        root.set_size_request(WIDTH, -1);
        root.set_hexpand(false);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        actions.add_css_class("pigeon-sidebar-top");
        let connection = widgets::label("", "pigeon-quiet");
        connection.set_hexpand(true);
        actions.append(&connection);
        let show = widgets::icon_button("view-list-symbolic", "Status Records…");
        let collapse = widgets::icon_button("sidebar-show-symbolic", "Toggle Sidebar");
        let new = widgets::icon_button("list-add-symbolic", "New Session");
        actions.append(&show);
        actions.append(&collapse);
        actions.append(&new);
        root.append(&actions);
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::None);
        let scroll = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .child(&list)
            .build();
        root.append(&scroll);
        let records = RecordsView::new(workspace);
        workspace.borrow().root.add_overlay(&records.root);
        let view = Rc::new(Self {
            root,
            list,
            connection,
            records,
            work: Rc::downgrade(workspace),
            projects: RefCell::new(HashMap::new()),
            signature: RefCell::new(String::new()),
        });
        let work = Rc::downgrade(workspace);
        new.connect_clicked(move |_| {
            if let Some(work) = work.upgrade() {
                work.borrow_mut().new_session();
                let area = work.borrow().area.clone();
                area.grab_focus();
            }
        });
        let weak = Rc::downgrade(&view);
        show.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                if let Some(work) = view.work.upgrade() {
                    view.records.open(&work.borrow());
                }
            }
        });
        let weak = Rc::downgrade(&view);
        collapse.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.root.set_visible(false);
            }
        });
        view
    }

    fn close_dialog(root: &gtk::Box, weak: &Weak<RefCell<Workspace>>) {
        let Some(work) = weak.upgrade() else {
            return;
        };
        let Some(session) = work.borrow().model.selected else {
            return;
        };
        let Some(window) = root.root().and_downcast::<gtk::Window>() else {
            return;
        };
        let dialog = gtk::MessageDialog::builder()
            .transient_for(&window)
            .modal(true)
            .text("Close this session?")
            .secondary_text("All panes and their running processes in this session will be closed.")
            .build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Close Session", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Cancel);
        let work = weak.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(work) = work.upgrade() {
                    work.borrow_mut().close_session(session);
                }
            }
            dialog.close();
            if let Some(work) = work.upgrade() {
                let area = work.borrow().area.clone();
                area.grab_focus();
            }
        });
        dialog.present();
    }

    pub fn refresh(&self) {
        let Some(work) = self.work.upgrade() else {
            return;
        };
        let work = work.borrow();
        self.records.refresh(&work);
        self.connection.set_text(if work.model.connected {
            ""
        } else {
            &work.disconnected_reason
        });
        self.connection
            .set_ellipsize(gtk::pango::EllipsizeMode::End);
        self.connection.set_tooltip_text(
            (!work.model.connected)
                .then(|| format!("{} · restart to connect", work.disconnected_reason))
                .as_deref(),
        );
        self.projects
            .borrow_mut()
            .retain(|cwd, _| work.model.sessions.iter().any(|s| &s.cwd == cwd));
        for session in &work.model.sessions {
            self.projects
                .borrow_mut()
                .entry(session.cwd.clone())
                .or_insert_with(|| {
                    let git = Path::new(&session.cwd)
                        .ancestors()
                        .find(|path| path.join(".git").exists())
                        .and_then(Path::to_str);
                    grouping::project(&session.cwd, git)
                });
        }
        let rows = work
            .status
            .rows(&work.model.sessions, &self.projects.borrow(), &work.sizes);
        let signature = format!("{:?}{rows:?}", work.model.selected);
        if *self.signature.borrow() == signature {
            return;
        }
        *self.signature.borrow_mut() = signature;
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        let mut first = true;
        for row in rows {
            if let Some(session) = row.session {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                line.add_css_class("pigeon-session");
                if work.model.selected == Some(session) {
                    line.add_css_class("selected");
                }
                let button = widgets::named_button(&row.text);
                button.set_hexpand(true);
                button.add_css_class("pigeon-session-button");
                let inner = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let title = widgets::label(&row.name, "pigeon-session-title");
                title.set_hexpand(true);
                title.set_ellipsize(gtk::pango::EllipsizeMode::End);
                title.set_max_width_chars(24);
                if row.unread {
                    title.add_css_class("unread");
                }
                let age = widgets::label(&row.age, "pigeon-age");
                inner.append(&title);
                inner.append(&age);
                button.set_child(Some(&inner));
                button.set_tooltip_text(Some(&row.tooltip));
                let weak = self.work.clone();
                button.connect_clicked(move |_| {
                    if let Some(work) = weak.upgrade() {
                        work.borrow_mut().select(session);
                        let area = work.borrow().area.clone();
                        area.grab_focus();
                    }
                });
                line.append(&button);
                let slot = gtk::Stack::new();
                slot.set_size_request(24, 24);
                slot.set_margin_end(5);
                let status = widgets::status(row.state, row.unread);
                slot.add_named(&status, Some("status"));
                let close = widgets::named_button("Close Session…");
                close.set_icon_name("window-close-symbolic");
                close.add_css_class("pigeon-close");
                close.set_tooltip_text(Some("Close Session…"));
                close.set_widget_name(&format!("close-session-{session}"));
                slot.add_named(&close, Some("close"));
                let weak = self.work.clone();
                let parent = self.root.downgrade();
                close.connect_clicked(move |_| {
                    if let (Some(work), Some(parent)) = (weak.upgrade(), parent.upgrade()) {
                        work.borrow_mut().select(session);
                        Self::close_dialog(&parent, &weak);
                    }
                });
                let motion = gtk::EventControllerMotion::new();
                let stack = slot.clone();
                motion.connect_enter(move |_, _, _| stack.set_visible_child_name("close"));
                let stack = slot.clone();
                motion.connect_leave(move |_| stack.set_visible_child_name("status"));
                line.add_controller(motion);
                line.append(&slot);
                self.list.append(&line);
            } else {
                let group = gtk::Box::new(gtk::Orientation::Horizontal, 5);
                group.add_css_class("pigeon-project");
                if first {
                    group.add_css_class("first");
                    first = false;
                }
                let name = widgets::label(&row.name, "pigeon-section-name");
                name.set_ellipsize(gtk::pango::EllipsizeMode::End);
                group.append(&name);
                group.append(&widgets::label(&row.count.to_string(), "pigeon-count"));
                let spacer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
                spacer.set_hexpand(true);
                group.append(&spacer);
                if row.needs > 0 {
                    group.append(&widgets::label(
                        &format!("{} needs you", row.needs),
                        "pigeon-needs",
                    ));
                }
                group.set_tooltip_text(Some(&row.tooltip));
                self.list.append(&group);
            }
        }
    }

    pub fn install_keys(self: &Rc<Self>, window: &gtk::ApplicationWindow) {
        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            use gtk::gdk::{Key, ModifierType as M};
            if !modifiers.contains(M::CONTROL_MASK | M::SHIFT_MASK)
                || modifiers.intersects(M::ALT_MASK | M::SUPER_MASK | M::META_MASK)
            {
                return gtk::glib::Propagation::Proceed;
            }
            let Some(view) = weak.upgrade() else {
                return gtk::glib::Propagation::Proceed;
            };
            let Some(work) = view.work.upgrade() else {
                return gtk::glib::Propagation::Proceed;
            };
            match key.to_lower() {
                Key::t => work.borrow_mut().new_session(),
                Key::b => {
                    view.root.set_visible(!view.root.is_visible());
                    let area = work.borrow().area.clone();
                    area.grab_focus();
                }
                Key::j => {
                    let target = {
                        let work = work.borrow();
                        work.status
                            .next_needed(&work.model.sessions, work.model.selected)
                    };
                    if let Some(target) = target {
                        work.borrow_mut().select(target);
                        view.root.set_visible(true);
                        view.records.open(&work.borrow());
                    }
                }
                Key::bracketleft | Key::bracketright | Key::braceleft | Key::braceright => {
                    let target = {
                        let work = work.borrow();
                        rows::adjacent(
                            &work.model.sessions,
                            work.model.selected,
                            matches!(key.to_lower(), Key::bracketleft | Key::braceleft),
                        )
                    };
                    if let Some(target) = target {
                        work.borrow_mut().select(target);
                        let area = work.borrow().area.clone();
                        area.grab_focus();
                    }
                }
                _ => return gtk::glib::Propagation::Proceed,
            }
            gtk::glib::Propagation::Stop
        });
        window.add_controller(controller);
    }
}
