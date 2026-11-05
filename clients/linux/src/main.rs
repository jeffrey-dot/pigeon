//! pigeon for Linux: a GTK4 window with an OpenGL grid renderer, talking the host protocol
//! through `pigeon_core::api`.
//!
//! Scope so far (`docs/renderer.md` §5): connect to the local host, draw every pane of the
//! session at its rect, handle IME/keys/clipboard, follow layout changes and window resizes.
//! Scrollback slices and complete status records come from the host.

mod host;
mod input;
mod input_context;
mod placement;
mod render;
mod screen;
mod scroll;
mod scroll_input;
mod session_model;
mod sidebar;
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
    let metrics = view::measure(scale);
    let width = 80;
    let height = 24;
    let workspace = Workspace::new(&hello, host.clone(), scale);

    let sidebar = sidebar::view::SidebarView::new(&workspace);
    let content = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    content.append(&sidebar.root);
    content.append(&workspace.borrow().root);
    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("pigeon")
        .child(&content)
        .build();
    // The application holds the window: without this the local goes out of scope when
    // `start` returns and GTK destroys the just-presented window.
    application.add_window(&window);
    // Match a new host pane initially; existing sessions resize after their first layout.
    if let Some(metrics) = metrics {
        let w = (width as f32 * metrics.cell_w / scale).round().max(80.0);
        let h = (height as f32 * metrics.cell_h / scale).round().max(60.0);
        window.set_default_size(w as i32 + sidebar::view::WIDTH + 16, h as i32);
    }
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
    workspace.borrow().area.grab_focus();

    let receiver = host.receiver();
    let host_for_pump = host.clone();
    let workspace_for_pump = workspace.clone();
    gtk::glib::spawn_future_local(async move {
        while let Ok(message) = receiver.recv().await {
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
        eprintln!("pigeon: the host closed the connection");
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
