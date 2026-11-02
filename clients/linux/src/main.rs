//! pigeon for Linux: a GTK4 window with an OpenGL grid renderer, talking the host protocol
//! through `pigeon_core::api`.
//!
//! Scope of this first slice (`docs/renderer.md` §5.5): connect to the local host, show one
//! pane, send keys, follow window resizes. The sidebar, splits and scrollback come next.

mod host;
mod input;
mod pane;
mod render;
mod screen;

use gtk::prelude::*;
use gtk4 as gtk;
use pigeon_core::api::Message;

use crate::pane::Pane;

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
    let metrics = render::measure(scale);
    let pane = Pane::new(screen, host.clone(), scale);
    let (cols, rows) = pane.borrow().grid();

    let window = gtk::ApplicationWindow::builder()
        .application(application)
        .title("pigeon")
        .child(&pane.borrow().area)
        .build();
    // The application holds the window: without this the local goes out of scope when
    // `start` returns and GTK destroys the just-presented window.
    application.add_window(&window);
    // Open at the size the host's screen already has, so the first frame is not a resize.
    if let Some(metrics) = metrics {
        let width = (cols as f32 * metrics.cell_w / scale).round().max(80.0);
        let height = (rows as f32 * metrics.cell_h / scale).round().max(60.0);
        window.set_default_size(width as i32, height as i32);
    }
    window.present();
    pane.borrow().area.grab_focus();

    let receiver = host.receiver();
    let host_for_pump = host.clone();
    let pane_for_pump = pane.clone();
    gtk::glib::spawn_future_local(async move {
        while let Ok(message) = receiver.recv().await {
            match &message {
                Message::Ping { ts } => host_for_pump.send(Message::Pong { ts: *ts }),
                Message::Unsupported { t_of } => {
                    eprintln!("pigeon: host does not know {t_of:?}");
                }
                Message::Error { code, detail, .. } => {
                    eprintln!("pigeon: host error {code}: {detail}");
                    // The outbound queue overflowed: the screen is behind, so re-sync
                    // instead of waiting for frames that were dropped (protocol §7).
                    if code == "backpressure" {
                        reattach(&pane_for_pump, &host_for_pump);
                    }
                }
                other => {
                    let handled = pane_for_pump.borrow_mut().apply(other);
                    if !handled {
                        reattach(&pane_for_pump, &host_for_pump);
                    }
                }
            }
        }
        eprintln!("pigeon: the host closed the connection");
    });
}

/// Asks for a fresh snapshot for this pane; the client keeps drawing the old screen until it
/// arrives, so the window never goes blank mid-session.
fn reattach(pane: &std::rc::Rc<std::cell::RefCell<Pane>>, host: &host::HostLink) {
    let pane_id = pane.borrow().pane_id();
    host.send(Message::Attach {
        id: u32::MAX,
        pane_id,
        size_policy: None,
        history: Some(false),
        since: None,
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
