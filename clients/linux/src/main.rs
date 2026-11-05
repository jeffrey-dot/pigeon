//! pigeon for Linux: a GTK4 window with an OpenGL grid renderer, talking the host protocol
//! through `pigeon_core::api`.
//!
//! Scope so far (`docs/renderer.md` §5): connect to the local host, draw every pane of the
//! session at its rect, handle IME/keys/clipboard, follow layout changes and window resizes.
//! Scrollback slices and complete status records come from the host.

mod host;
mod input;
mod input_context;
mod keyboard;
mod links;
mod placement;
mod pointer;
mod render;
mod screen;
mod scroll;
mod scroll_input;
mod search;
mod session_model;
mod shell;
mod sidebar;
mod theme;
mod view;

use gtk::prelude::*;
use gtk4 as gtk;
use pigeon_core::api::Message;

use crate::view::Workspace;

const APP_ID: &str = "dev.pigeon.Linux";

fn main() -> gtk::glib::ExitCode {
    let application = gtk::Application::builder().application_id(APP_ID).build();
    application.connect_activate(start);
    application.run()
}

fn start(application: &gtk::Application) {
    theme::install();
    let host = match host::HostLink::connect() {
        Ok(host) => host,
        Err(error) => {
            eprintln!("pigeon: {error}");
            application.quit();
            return;
        }
    };
    let hello = match host::handshake(&host) {
        Ok(hello) => hello,
        Err(error) => {
            eprintln!("pigeon: {error}");
            application.quit();
            return;
        }
    };

    let scale = primary_scale();
    let workspace = Workspace::new(&hello, host.clone(), scale);
    workspace.borrow_mut().set_appearance(theme::dark());

    let sidebar = sidebar::view::SidebarView::new(&workspace);
    let window = shell::window(application, &workspace, &sidebar);
    // The application holds the window: without this the local goes out of scope when
    // `start` returns and GTK destroys the just-presented window.
    application.add_window(&window);
    sidebar.install_keys(&window);
    let sidebar_for_timer = sidebar.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        sidebar_for_timer.refresh();
        gtk::glib::ControlFlow::Continue
    });
    // The current host broadcasts metadata changes, but not every background tree removal.
    let refresh_host = host.clone();
    gtk::glib::timeout_add_local(std::time::Duration::from_secs(2), move || {
        refresh_host.send(Message::Sessions { items: vec![] });
        gtk::glib::ControlFlow::Continue
    });
    window.present();
    let area = workspace.borrow().area.clone();
    area.grab_focus();

    let receiver = host.clone();
    let host_for_pump = host.clone();
    let workspace_for_pump = workspace.clone();
    gtk::glib::spawn_future_local(async move {
        while let Some(message) = receiver.next().await {
            match &message {
                Message::Ping { ts } => host_for_pump.send(Message::Pong { ts: *ts }),
                Message::Unsupported { t_of } => {
                    eprintln!("pigeon: host does not know {t_of:?}");
                }
                Message::Error { code, detail, .. } => {
                    eprintln!("pigeon: host error {code}: {detail}");
                    workspace_for_pump.borrow_mut().apply(&message);
                }
                other => {
                    // A pane that is out of step asks for a fresh snapshot; the window keeps
                    // drawing the old one until it arrives.
                    workspace_for_pump.borrow_mut().apply(other);
                }
            }
        }
        workspace_for_pump.borrow_mut().disconnect();
        eprintln!("pigeon: {}", receiver.reason());
    });
}

/// The scale of the primary monitor, before any window exists.
fn primary_scale() -> f32 {
    gtk::gdk::Display::default()
        .and_then(|display| display.monitors().item(0))
        .and_then(|monitor| monitor.downcast::<gtk::gdk::Monitor>().ok())
        .map(|monitor| monitor.scale_factor() as f32)
        .unwrap_or(1.0)
}
