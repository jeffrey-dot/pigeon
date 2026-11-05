//! pigeon for GPUI: one GPU-rendered client across macOS, Linux and Windows, talking the
//! host protocol through `pigeon_core::api`.
//!
//! The terminal VT semantics, snapshot/delta, layout, selection, OSC 7501 records, search and
//! protocol codec all live in `core/` behind `pigeon_core::api` (AGENTS.md §3); this client
//! only draws and maps input. It replaces the per-platform AppKit/GTK clients with a single
//! GPUI UI — see docs/adr/0013-gpui-single-client.md and docs/gui.md.
//!
//! Stage 1: connect to the local host, hand the session list to the view, and draw the Harp
//! shell at the reference sizes/colours (docs/ui.md). The terminal grid renderer is next.

mod host;
mod icons;
mod sidebar;
mod status;
mod terminal;
mod theme;
mod view;

use gpui::{
    point, px, size, App, AppContext, Bounds, TitlebarOptions, WindowBounds, WindowOptions,
};
use std::time::Duration;

use crate::host::{handshake, HostLink};
use crate::theme::metrics;
use crate::view::Workspace;

fn main() {
    // The handshake runs before any window: a window with no host has nothing to show, and
    // this keeps startup failures as a plain stderr message rather than an empty window.
    let host = match HostLink::connect() {
        Ok(host) => host,
        Err(error) => {
            eprintln!("pigeon: {error}");
            std::process::exit(1);
        }
    };
    let hello = match handshake(&host) {
        Ok(hello) => hello,
        Err(error) => {
            eprintln!("pigeon: {error}");
            std::process::exit(1);
        }
    };

    gpui_platform::application().run(move |cx: &mut App| {
        let (width, height) = window_size();
        let bounds = Bounds::centered(None, size(px(width), px(height)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                // Hidden title + transparent titlebar (docs/ui.md §2); the OS keeps the window
                // controls, centred on the sidebar's 56 pt top bar.
                title: None,
                appears_transparent: true,
                traffic_light_position: Some(point(
                    px(metrics::TRAFFIC_LIGHT_X),
                    px(metrics::TRAFFIC_LIGHT_Y),
                )),
            }),
            ..Default::default()
        };

        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| {
                let dark = theme::dark_for(window.appearance());
                // Follow system appearance changes while the window is open.
                cx.observe_window_appearance(window, |workspace: &mut Workspace, window, cx| {
                    workspace.set_dark(theme::dark_for(window.appearance()));
                    cx.notify();
                })
                .detach();
                Workspace::new(host.clone(), dark, &hello)
            })
        });
        let window = match opened {
            Ok(window) => window,
            Err(error) => {
                eprintln!("pigeon: cannot open a window: {error}");
                cx.quit();
                return;
            }
        };

        let _ = window.update(cx, |workspace, _window, _cx| workspace.request_sessions());

        // Host messages into the view, on GPUI's foreground executor.
        let pump_host = host.clone();
        cx.spawn(async move |cx| loop {
            match pump_host.next().await {
                Some(message) => {
                    let update = window.update(cx, |workspace, _window, cx| {
                        workspace.ingest(&message);
                        cx.notify();
                    });
                    if update.is_err() {
                        break; // Window closed.
                    }
                }
                None => {
                    let reason = pump_host.reason();
                    let _ = window.update(cx, |workspace, _window, cx| {
                        workspace.disconnected(reason);
                        cx.notify();
                    });
                    break;
                }
            }
        })
        .detach();

        // Relative times (`now` / `1m` / ...) advance once a minute (docs/ui.md §3.2).
        cx.spawn(async move |cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(60))
                .await;
            if window.update(cx, |_, _, cx| cx.notify()).is_err() {
                break;
            }
        })
        .detach();

        cx.activate(true);
    });
}

/// The initial window size in points. `PIGEON_WINDOW_SIZE=WxH` exists for the screenshot check
/// (tools/checks/gpui-ui-check.py), which opens the window at the reference image's size; it
/// never goes below the minimum content size (docs/ui.md §2).
fn window_size() -> (f32, f32) {
    let minimum = (
        metrics::MIN_CONTENT_WIDTH + metrics::SIDEBAR_WIDTH,
        metrics::MIN_CONTENT_HEIGHT,
    );
    let requested = std::env::var("PIGEON_WINDOW_SIZE").ok().and_then(|text| {
        let (w, h) = text.split_once('x')?;
        Some((w.trim().parse::<f32>().ok()?, h.trim().parse::<f32>().ok()?))
    });
    match requested {
        Some((w, h)) if w.is_finite() && h.is_finite() => (w.max(minimum.0), h.max(minimum.1)),
        _ => minimum,
    }
}
