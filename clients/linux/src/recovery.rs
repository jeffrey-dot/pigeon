//! One sequential connection pump; blocking reconnect/hello run outside GTK's main thread.
use crate::{host::HostLink, view::Workspace};
use gtk4::{self as gtk, glib, prelude::*};
use pigeon_core::api::Message;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::Duration,
};

pub fn install(work: &Rc<RefCell<Workspace>>, window: &gtk::ApplicationWindow, mut link: HostLink) {
    let alive = Rc::new(Cell::new(true));
    let running = alive.clone();
    let weak = Rc::downgrade(work);
    window.connect_close_request(move |_| {
        running.set(false);
        if let Some(work) = weak.upgrade() {
            work.borrow().stop_connection();
        }
        glib::Propagation::Proceed
    });
    let weak = Rc::downgrade(work);
    let keepalive = work.clone();
    glib::spawn_future_local(async move {
        let _keepalive = keepalive;
        let endpoint = link.endpoint();
        loop {
            let Some(work) = weak.upgrade() else {
                link.close();
                return;
            };
            let generation = work.borrow().connection_generation;
            drop(work);
            while let Some(message) = link.next().await {
                if !alive.get() {
                    link.close();
                    return;
                }
                if !link.connected() {
                    break;
                }
                let Some(work) = weak.upgrade() else {
                    link.close();
                    return;
                };
                if work.borrow().connection_generation != generation {
                    return;
                }
                match message {
                    Message::Ping { ts } => link.send(Message::Pong { ts }),
                    Message::Unsupported { ref t_of } => {
                        eprintln!("pigeon: host does not know {t_of:?}")
                    }
                    _ => work.borrow_mut().apply(&message),
                }
            }
            if !alive.get() {
                return;
            }
            let Some(work) = weak.upgrade() else {
                return;
            };
            work.borrow_mut().disconnect();
            drop(work);
            let mut attempt = 0u32;
            loop {
                if !alive.get() {
                    return;
                }
                if let Some(work) = weak.upgrade() {
                    work.borrow_mut().disconnected_reason =
                        format!("Reconnecting… · {}", link.reason());
                } else {
                    return;
                }
                glib::timeout_future(Duration::from_secs(1u64 << attempt.min(4))).await;
                if !alive.get() {
                    return;
                }
                let (sender, receiver) = async_channel::bounded(1);
                let socket = endpoint.clone();
                std::thread::spawn(move || {
                    let result = HostLink::reconnect(&socket);
                    if let Err(error) = sender.send_blocking(result) {
                        if let Ok((link, _)) = error.0 {
                            link.close();
                        }
                    }
                });
                match receiver.recv().await {
                    Ok(Ok((next, hello))) => {
                        if !alive.get() {
                            next.close();
                            return;
                        }
                        let Some(work) = weak.upgrade() else {
                            next.close();
                            return;
                        };
                        work.borrow_mut().reconnected(next.clone(), &hello);
                        link = next;
                        break;
                    }
                    Ok(Err(error)) => {
                        eprintln!("pigeon: {error}");
                        if error.starts_with("host refused:") || error.starts_with("host speaks") {
                            if let Some(work) = weak.upgrade() {
                                work.borrow_mut().disconnected_reason = error;
                            }
                            return;
                        }
                    }
                    Err(_) => return,
                }
                attempt = attempt.saturating_add(1);
            }
        }
    });
}
