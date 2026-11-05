//! Explicit per-record inspection. Opening or selecting never acknowledges anything.
use super::RecordRow;
use crate::view::Workspace;
use gtk4::{self as gtk, prelude::*};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

pub struct RecordsView {
    pub root: gtk::Box,
    chooser: gtk::ComboBoxText,
    text: gtk::TextView,
    ack: gtk::Button,
    bound: Cell<Option<(u32, u64)>>,
    selected: RefCell<Option<RecordRow>>,
    rows: RefCell<Vec<RecordRow>>,
    updating: Cell<bool>,
}

impl RecordsView {
    pub fn new(workspace: &Rc<RefCell<Workspace>>) -> Rc<Self> {
        let root = gtk::Box::new(gtk::Orientation::Vertical, 6);
        root.set_hexpand(false);
        root.set_visible(false);
        let heading = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        let label = gtk::Label::new(Some("Status Records"));
        label.set_hexpand(true);
        label.set_xalign(0.0);
        heading.append(&label);
        let close = gtk::Button::with_label("×");
        close.set_tooltip_text(Some("Close status records"));
        heading.append(&close);
        root.append(&heading);
        let chooser = gtk::ComboBoxText::new();
        for cell in chooser.cells() {
            cell.set_property("ellipsize", gtk::pango::EllipsizeMode::End);
            cell.set_property("max-width-chars", 28i32);
        }
        root.append(&chooser);
        let text = gtk::TextView::new();
        text.set_editable(false);
        text.set_cursor_visible(false);
        text.set_wrap_mode(gtk::WrapMode::WordChar);
        let scroll = gtk::ScrolledWindow::builder()
            .child(&text)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .min_content_height(100)
            .vexpand(true)
            .build();
        root.append(&scroll);
        let ack = gtk::Button::with_label("Mark This Record as Read");
        ack.set_sensitive(false);
        root.append(&ack);
        let view = Rc::new(Self {
            root,
            chooser,
            text,
            ack,
            bound: Cell::new(None),
            selected: RefCell::new(None),
            rows: RefCell::new(vec![]),
            updating: Cell::new(false),
        });
        let weak = Rc::downgrade(&view);
        close.connect_clicked(move |_| {
            if let Some(view) = weak.upgrade() {
                view.hide();
            }
        });
        let weak = Rc::downgrade(&view);
        let work = Rc::downgrade(workspace);
        view.chooser.connect_changed(move |chooser| {
            let (Some(view), Some(work)) = (weak.upgrade(), work.upgrade()) else {
                return;
            };
            if view.updating.get() {
                return;
            }
            let row = chooser
                .active()
                .and_then(|index| index.checked_sub(1))
                .and_then(|index| view.rows.borrow().get(index as usize).cloned());
            *view.selected.borrow_mut() = row.clone();
            if let Some(row) = row {
                work.borrow_mut().reveal(row.pane);
            }
            view.refresh(&work.borrow());
        });
        let weak = Rc::downgrade(&view);
        let work = Rc::downgrade(workspace);
        view.ack.connect_clicked(move |_| {
            let (Some(view), Some(work)) = (weak.upgrade(), work.upgrade()) else {
                return;
            };
            let Some(row) = view.selected.borrow().clone() else {
                return;
            };
            let visible = view.visible(&work.borrow(), &row);
            let message = work.borrow_mut().status.ack(&row, visible);
            if let Some(message) = message {
                work.borrow().send_effects(vec![message]);
            }
            view.refresh(&work.borrow());
        });
        view
    }

    fn hide(&self) {
        self.root.set_visible(false);
        self.bound.set(None);
        self.selected.borrow_mut().take();
    }

    pub fn open(&self, workspace: &Workspace) {
        self.bound.set(
            workspace
                .model
                .selected
                .map(|session| (session, workspace.model.selection_generation)),
        );
        self.selected.borrow_mut().take();
        self.rows.borrow_mut().clear();
        self.updating.set(true);
        self.chooser.remove_all();
        self.chooser.append_text("Select a record…");
        self.chooser.set_active(Some(0));
        self.updating.set(false);
        self.root.set_visible(true);
        self.refresh(workspace);
        let chooser = self.chooser.clone();
        gtk::glib::idle_add_local_once(move || {
            chooser.grab_focus();
        });
    }

    fn visible(&self, workspace: &Workspace, row: &RecordRow) -> bool {
        self.root.is_mapped()
            && self.bound.get()
                == workspace
                    .model
                    .selected
                    .map(|session| (session, workspace.model.selection_generation))
            && self
                .root
                .root()
                .and_downcast::<gtk::Window>()
                .is_some_and(|window| {
                    window.is_active()
                        && window.is_visible()
                        && window
                            .surface()
                            .and_downcast::<gtk::gdk::Toplevel>()
                            .is_some_and(|surface| {
                                !surface.state().contains(gtk::gdk::ToplevelState::MINIMIZED)
                            })
                })
            && workspace.record_visible(row.pane)
    }

    pub fn refresh(&self, workspace: &Workspace) {
        if !self.root.is_visible() {
            return;
        }
        if self.bound.get()
            != workspace
                .model
                .selected
                .map(|session| (session, workspace.model.selection_generation))
            || !workspace.model.connected
        {
            self.hide();
            let area = workspace.area.clone();
            gtk::glib::idle_add_local_once(move || {
                area.grab_focus();
            });
            return;
        }
        let rows = workspace
            .model
            .selected_session()
            .map(|s| workspace.status.records(&s.panes))
            .unwrap_or_default();
        if self
            .selected
            .borrow()
            .as_ref()
            .is_some_and(|r| !workspace.status.current(r))
        {
            self.selected.borrow_mut().take();
        }
        // age_ms samples may change independently of identity; preserve only an exact selection.
        if *self.rows.borrow() != rows {
            self.updating.set(true);
            self.chooser.remove_all();
            self.chooser.append_text("Select a record…");
            for row in &rows {
                let id = if row.record.id.is_empty() {
                    "default"
                } else {
                    &row.record.id
                };
                self.chooser.append_text(&format!(
                    "Pane {} · {} · {}",
                    row.pane,
                    id,
                    row.record.state.as_str()
                ));
            }
            let index = self.selected.borrow().as_ref().and_then(|selected| {
                rows.iter().position(|r| {
                    r.pane == selected.pane
                        && r.record.id == selected.record.id
                        && r.record.seq == selected.record.seq
                })
            });
            self.chooser
                .set_active(Some(index.map_or(0, |i| i as u32 + 1)));
            *self.rows.borrow_mut() = rows;
            self.updating.set(false);
        }
        let selected = self.selected.borrow();
        let detail = selected.as_ref().map(|row| format!("Pane {} · {}\nApp: {}\nKind: {}\n\n{}", row.pane, row.record.state.as_str(), row.record.app, row.record.kind.as_str(), row.record.msg))
            .unwrap_or_else(|| "Select a record to read its full message. Only done/error can be marked as read while its pane is visible and live.".into());
        if self
            .text
            .buffer()
            .text(
                &self.text.buffer().start_iter(),
                &self.text.buffer().end_iter(),
                false,
            )
            .as_str()
            != detail
        {
            self.text.buffer().set_text(&detail);
        }
        self.ack.set_sensitive(
            selected
                .as_ref()
                .is_some_and(|row| workspace.status.can_ack(row, self.visible(workspace, row))),
        );
    }
}
