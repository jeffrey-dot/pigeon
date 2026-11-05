//! Native window chrome around the terminal; all terminal content remains a real grid.
use crate::{
    sidebar::{rows, view::SidebarView},
    theme,
    view::Workspace,
};
use gtk4::{self as gtk, prelude::*};
use std::{cell::RefCell, rc::Rc};

pub fn window(
    application: &gtk::Application,
    workspace: &Rc<RefCell<Workspace>>,
    sidebar: &Rc<SidebarView>,
) -> gtk::ApplicationWindow {
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.append(&sidebar.root);
    let card = gtk::Box::new(gtk::Orientation::Vertical, 0);
    card.add_css_class("pigeon-card");
    card.set_overflow(gtk::Overflow::Hidden);
    card.set_margin_top(6);
    card.set_margin_end(8);
    card.set_margin_bottom(8);
    card.set_hexpand(true);
    card.set_vexpand(true);
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    header.add_css_class("pigeon-terminal-header");
    let reveal = gtk::Button::from_icon_name("sidebar-show-symbolic");
    reveal.add_css_class("pigeon-icon");
    reveal.set_tooltip_text(Some("Toggle Sidebar"));
    let side = sidebar.root.clone();
    reveal.connect_clicked(move |_| side.set_visible(!side.is_visible()));
    header.append(&reveal);
    let title = gtk::Label::new(Some("Terminal"));
    title.add_css_class("pigeon-header-title");
    title.set_ellipsize(gtk::pango::EllipsizeMode::End);
    title.set_max_width_chars(28);
    let path = gtk::Label::new(None);
    path.add_css_class("pigeon-header-path");
    path.set_ellipsize(gtk::pango::EllipsizeMode::Start);
    path.set_hexpand(true);
    path.set_xalign(0.0);
    let state = gtk::Label::new(None);
    state.add_css_class("pigeon-header-state");
    header.append(&title);
    header.append(&path);
    header.append(&state);
    card.append(&header);
    let terminal = gtk::Box::new(gtk::Orientation::Vertical, 0);
    terminal.add_css_class("pigeon-terminal");
    terminal.set_vexpand(true);
    terminal.append(&workspace.borrow().root);
    card.append(&terminal);
    content.append(&card);
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("pigeon")
        .default_width(
            crate::view::measure(1.0)
                .map_or(1100, |m| (m.cell_w * 100.0) as i32 + 320)
                .max(1100),
        )
        .default_height(720)
        .child(&content)
        .build();
    let bar = gtk::HeaderBar::new();
    bar.set_title_widget(Some(&gtk::Box::new(gtk::Orientation::Horizontal, 0)));
    window.set_titlebar(Some(&bar));
    theme::apply(&window, theme::dark());
    header.append(&crate::search::view::install(workspace, &window));
    let work = Rc::downgrade(workspace);
    let side = sidebar.root.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        let Some(work) = work.upgrade() else {
            return gtk::glib::ControlFlow::Break;
        };
        let work = work.borrow();
        reveal.set_visible(!side.is_visible());
        if let Some(session) = work.model.selected_session() {
            title.set_text(
                session
                    .active_title
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(&session.title),
            );
            path.set_text(&session.cwd);
            path.set_tooltip_text(Some(&session.cwd));
            let summary = work.status.summary(&session.panes);
            let text = summary
                .state
                .map(|s| {
                    let word = s.as_str();
                    let mut chars = word.chars();
                    let capital = chars
                        .next()
                        .map(|c| c.to_uppercase().to_string())
                        .unwrap_or_default()
                        + chars.as_str();
                    format!("{capital} · {}", rows::age(summary.age_ms.unwrap_or(0)))
                })
                .unwrap_or_default();
            state.set_text(&text);
        } else {
            title.set_text("Terminal");
            path.set_text("");
            state.set_text("");
        }
        gtk::glib::ControlFlow::Continue
    });
    if let Some(settings) = gtk::Settings::default() {
        let weak = window.downgrade();
        let work = Rc::downgrade(workspace);
        settings.connect_gtk_application_prefer_dark_theme_notify(move |_| {
            if let (Some(window), Some(work)) = (weak.upgrade(), work.upgrade()) {
                let dark = theme::dark();
                theme::apply(&window, dark);
                work.borrow_mut().set_appearance(dark);
            }
        });
    }
    window
}
