//! pigeon for Linux: a GTK4 window with an OpenGL grid renderer, talking the host protocol
//! through `pigeon_core::api`.
//!
//! Scope so far (`docs/renderer.md` §5): connect to the local host, draw every pane of the
//! session at its rect, handle IME/keys/clipboard, follow layout changes and window resizes.
//! The sidebar and scrollback come next.

mod host;
mod input;
mod input_context;
mod placement;
mod render;
mod screen;
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
    let screen = match host::handshake(&host) {
        Ok(screen) => screen,
        Err(error) => {
            eprintln!("pigeon: {error}");
            application.quit();
            return;
        }
    };

    let scale = primary_scale();
    let metrics = view::measure(scale);
    let width = screen.cols();
    let height = screen.rows();
    let workspace = Workspace::new(screen, host.clone(), scale);

    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("pigeon")
        .child(&workspace.borrow().root)
        .build();
    // The application holds the window: without this the local goes out of scope when
    // `start` returns and GTK destroys the just-presented window.
    application.add_window(&window);
    // Open at the size the host's screen already has, so the first frame is not a resize.
    if let Some(metrics) = metrics {
        let w = (width as f32 * metrics.cell_w / scale).round().max(80.0);
        let h = (height as f32 * metrics.cell_h / scale).round().max(60.0);
        window.set_default_size(w as i32, h as i32);
    }
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
                }
                other => {
                    // A pane that is out of step asks for a fresh snapshot; the window keeps
                    // drawing the old one until it arrives.
                    if !workspace_for_pump.borrow_mut().apply(other) {
                        eprintln!("pigeon: out of step with the host, re-attaching");
                    }
                }
            }
        }
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
