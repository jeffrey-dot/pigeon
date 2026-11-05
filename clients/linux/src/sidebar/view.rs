//! GTK native local session list. Project discovery is client-side filesystem I/O.
use super::{grouping, priority, records::RecordsView};
use crate::view::Workspace;
use gtk4::{self as gtk, prelude::*};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
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
        let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
        root.set_size_request(WIDTH, -1);
        root.set_hexpand(false);
        root.set_halign(gtk::Align::Start);
        root.set_margin_start(8);
        root.set_margin_end(8);
        root.set_margin_top(8);
        root.set_margin_bottom(8);
        let connection = gtk::Label::new(Some("Local sessions"));
        connection.set_xalign(0.0);
        root.append(&connection);
        let actions = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let new = gtk::Button::with_label("New Session");
        new.set_tooltip_text(Some(
            "Ctrl+Shift+T · Create in the selected session's directory",
        ));
        actions.append(&new);
        let close = gtk::Button::with_label("Close Session…");
        actions.append(&close);
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
        let show = gtk::Button::with_label("Status Records…");
        root.append(&show);
        root.append(&records.root);
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
                work.borrow().area.grab_focus();
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
        close.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.confirm_close();
            }
        });
        view
    }

    fn confirm_close(&self) {
        let Some(work) = self.work.upgrade() else {
            return;
        };
        let Some(session) = work.borrow().model.selected else {
            return;
        };
        let Some(window) = self.root.root().and_downcast::<gtk::Window>() else {
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
        let work = self.work.clone();
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                if let Some(work) = work.upgrade() {
                    work.borrow_mut().close_session(session);
                }
            }
            dialog.close();
            if let Some(work) = work.upgrade() {
                work.borrow().area.grab_focus();
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
            "Local sessions"
        } else {
            "Disconnected · restart to connect"
        });
        self.projects
            .borrow_mut()
            .retain(|cwd, _| work.model.sessions.iter().any(|s| &s.cwd == cwd));
        let mut groups: BTreeMap<String, (String, Vec<_>)> = BTreeMap::new();
        for session in &work.model.sessions {
            let (key, name) = self
                .projects
                .borrow_mut()
                .entry(session.cwd.clone())
                .or_insert_with(|| {
                    let git = Path::new(&session.cwd)
                        .ancestors()
                        .find(|path| path.join(".git").exists())
                        .and_then(Path::to_str);
                    grouping::project(&session.cwd, git)
                })
                .clone();
            groups
                .entry(key)
                .or_insert_with(|| (name, vec![]))
                .1
                .push(session);
        }
        let mut rows = vec![];
        let mut signature = format!("{:?}", work.model.selected);
        for (path, (name, mut sessions)) in groups {
            sessions.sort_by_key(|s| {
                (
                    work.status
                        .summary(&s.panes)
                        .state
                        .map(priority)
                        .unwrap_or(5),
                    s.session_id,
                )
            });
            let needs = sessions
                .iter()
                .filter(|s| work.status.summary(&s.panes).needs_you)
                .count();
            let group = format!(
                "{name} · {} · {needs} {}",
                sessions.len(),
                if needs == 1 { "needs you" } else { "need you" }
            );
            signature.push_str(&group);
            rows.push((None, group, path));
            for session in sessions {
                let summary = work.status.summary(&session.panes);
                let state = summary.state.map(|s| s.as_str()).unwrap_or("");
                let age = summary.age_ms.map(age).unwrap_or_default();
                let unread = if summary.unread > 0 {
                    format!(" · {} unread", summary.unread)
                } else {
                    String::new()
                };
                let size = session
                    .panes
                    .iter()
                    .find_map(|p| work.sizes.get(p))
                    .map(|(source, clients)| format!(" · {clients}× {source}"))
                    .unwrap_or_default();
                let title = session
                    .active_program
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&session.title);
                let label = format!(
                    "{title} · #{}  {age} {state}{unread}{size}",
                    session.session_id
                );
                signature.push_str(&label);
                signature.push_str(&session.cwd);
                rows.push((
                    Some(session.session_id),
                    label,
                    format!("{}\n{state}{unread}{size}", session.cwd),
                ));
            }
        }
        if *self.signature.borrow() == signature {
            return;
        }
        *self.signature.borrow_mut() = signature;
        while let Some(child) = self.list.first_child() {
            self.list.remove(&child);
        }
        for (session, text, tooltip) in rows {
            if let Some(session) = session {
                let button = gtk::Button::new();
                let label = gtk::Label::new(Some(&text));
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::End);
                label.set_max_width_chars(32);
                button.set_child(Some(&label));
                button.set_tooltip_text(Some(&tooltip));
                if work.model.selected == Some(session) {
                    button.add_css_class("suggested-action");
                }
                let weak = self.work.clone();
                button.connect_clicked(move |_| {
                    if let Some(work) = weak.upgrade() {
                        work.borrow_mut().select(session);
                        work.borrow().area.grab_focus();
                    }
                });
                self.list.append(&button);
            } else {
                let label = gtk::Label::new(Some(&text));
                label.set_xalign(0.0);
                label.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
                label.set_tooltip_text(Some(&tooltip));
                label.set_margin_top(10);
                label.set_margin_bottom(4);
                self.list.append(&label);
            }
        }
    }

    pub fn install_keys(self: &Rc<Self>, window: &gtk::ApplicationWindow) {
        let controller = gtk::EventControllerKey::new();
        controller.set_propagation_phase(gtk::PropagationPhase::Capture);
        let weak = Rc::downgrade(self);
        controller.connect_key_pressed(move |_, key, _, modifiers| {
            use gtk::gdk::{Key, ModifierType as M};
            if !modifiers.contains(M::CONTROL_MASK | M::SHIFT_MASK) {
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
                    work.borrow().area.grab_focus();
                }
                Key::j => {
                    let target = {
                        let work = work.borrow();
                        let sessions = &work.model.sessions;
                        let current = sessions
                            .iter()
                            .position(|s| Some(s.session_id) == work.model.selected)
                            .unwrap_or(0);
                        (1..=sessions.len())
                            .map(|step| &sessions[(current + step) % sessions.len()])
                            .find(|s| work.status.summary(&s.panes).needs_you)
                            .map(|s| s.session_id)
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
                        let sessions = &work.model.sessions;
                        let current = sessions
                            .iter()
                            .position(|s| Some(s.session_id) == work.model.selected)
                            .unwrap_or(0);
                        if sessions.is_empty() {
                            None
                        } else {
                            let next =
                                if matches!(key.to_lower(), Key::bracketleft | Key::braceleft) {
                                    (current + sessions.len() - 1) % sessions.len()
                                } else {
                                    (current + 1) % sessions.len()
                                };
                            Some(sessions[next].session_id)
                        }
                    };
                    if let Some(target) = target {
                        work.borrow_mut().select(target);
                        work.borrow().area.grab_focus();
                    }
                }
                _ => return gtk::glib::Propagation::Proceed,
            }
            gtk::glib::Propagation::Stop
        });
        window.add_controller(controller);
    }
}

fn age(ms: u64) -> String {
    let minutes = ms / 60_000;
    if minutes == 0 {
        "now".into()
    } else if minutes < 60 {
        format!("{minutes}m")
    } else if minutes < 1440 {
        format!("{}h", minutes / 60)
    } else {
        format!("{}d", minutes / 1440)
    }
}
