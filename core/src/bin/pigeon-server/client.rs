//! One connection: decode frames into `dispatch`, and queue the host's events this
//! connection cares about for its writer (`outbound`).

use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tokio::time::{sleep_until, Instant};

use pigeon_core::error::PgError;
use pigeon_core::proto::{self, Message};

use crate::client_state::ClientState;
use crate::dispatch::dispatch;
use crate::event::Event;
use crate::handshake;
use crate::host_state::HostState;
use crate::outbound::Outbound;
use crate::pane_query::{latest_status, pending_statuses};
use crate::resync::resync;

/// Connection ids the host hands out in `hello_ack` (protocol §3.1).
static NEXT_CLIENT: AtomicU32 = AtomicU32::new(1);

pub async fn handle_client<S>(stream: S, host: HostState) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let client_id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    let (reader, writer) = tokio::io::split(stream);
    let (outbound, mut writer_task) = Outbound::start(writer, host.sessions.clone());
    let result = client_loop(reader, &host, client_id, &outbound, &mut writer_task).await;
    match &result {
        // A clean end may still have frames queued (a final `error`): let them out.
        Ok(()) => outbound.finish(writer_task).await,
        // Too slow, or the socket broke: nothing queued is worth waiting for.
        Err(_) => writer_task.abort(),
    }
    // A closed window must stop counting towards the session's size policy, or
    // `smallest` would stay pinned to a box nobody can see any more.
    host.forget_client(client_id).await;
    result
}

/// The reading half: frames and events in, everything out through `outbound`. Nothing here
/// waits for the socket to be writable.
async fn client_loop<R>(
    mut reader: R,
    host: &HostState,
    client_id: u32,
    outbound: &Outbound,
    writer_task: &mut JoinHandle<Result<(), String>>,
) -> Result<(), String>
where
    R: AsyncRead + Unpin,
{
    let mut inbox = Vec::new();
    let mut subscribed = host.events.subscribe();
    let mut state = ClientState::new(client_id);
    let connected = Instant::now();
    let mut last_heard = connected;

    loop {
        // Before `hello`: the hello timeout from connecting; after: the idle timeout from
        // the last inbound bytes (protocol §1, §3.1).
        let deadline = if state.greeted {
            last_heard + handshake::idle_timeout()
        } else {
            connected + handshake::hello_timeout()
        };
        tokio::select! {
            () = sleep_until(deadline) => {
                return Err(if state.greeted { "idle timeout" } else { "no hello" }.into());
            }
            read = reader.read_buf(&mut inbox) => {
                let count = read.map_err(|error| error.to_string())?;
                if count == 0 {
                    return Ok(());
                }
                last_heard = Instant::now();
                if !drain_inbox(&mut inbox, host, &mut state, outbound).await? {
                    return Ok(());
                }
            }
            event = subscribed.recv() => {
                match event {
                    Ok(event) => forward(event, host, &state, outbound).await?,
                    Err(RecvError::Lagged(missed)) => {
                        if crate::debug() {
                            eprintln!("pigeon-server: client {client_id} lagged by {missed} events");
                        }
                        // What is still queued predates the state about to be sent: skip it,
                        // so nothing older than the resync reaches the window after it.
                        subscribed = subscribed.resubscribe();
                        resync(host, &state, outbound).await?;
                    }
                    // The host holds a sender for as long as it runs.
                    Err(RecvError::Closed) => return Ok(()),
                }
            }
            // The writer only stops on its own when the socket broke.
            written = &mut *writer_task => {
                return match written {
                    Ok(result) => result.and(Err("connection closed while writing".into())),
                    Err(error) => Err(format!("writer: {error}")),
                };
            }
        }
    }
}

/// Handles every whole frame in `inbox`. Returns `false` when the connection must close
/// (a frame that does not decode at all).
async fn drain_inbox(
    inbox: &mut Vec<u8>,
    host: &HostState,
    state: &mut ClientState,
    outbound: &Outbound,
) -> Result<bool, String> {
    loop {
        let (decoded, used) = match proto::split_frame(inbox) {
            Ok(Some((payload, used))) => (proto::decode(payload), used),
            Ok(None) => return Ok(true),
            Err(error) => (Err(error), 0),
        };
        match decoded {
            Ok(message) => {
                inbox.drain(..used);
                if crate::debug() {
                    eprintln!("pigeon-server: <- {}", message.name());
                }
                if let Some(reply) = dispatch(message, host, state).await {
                    reply_to(reply, host, state, outbound).await?;
                }
                // Outside the reply branch: a replay has no single reply.
                for frame in std::mem::take(&mut state.outbox) {
                    outbound.send(&frame)?;
                }
                // A first frame that did not greet (`hello_error`, or not `hello` at all)
                // has had its answer; nothing else is served on this connection.
                if !state.greeted {
                    return Ok(false);
                }
            }
            // Protocol §2 rule 7: answer, skip exactly this frame, keep going.
            Err(PgError::Unsupported) => {
                let t_of = inbox.get(4..used).and_then(type_tag).unwrap_or_default();
                outbound.send(&Message::Unsupported { t_of })?;
                inbox.drain(..used);
            }
            Err(error) => {
                let detail = format!("{error}");
                let message = Message::Error {
                    code: "protocol".into(),
                    detail,
                    id: None,
                };
                outbound.send(&message)?;
                return Ok(false);
            }
        }
    }
}

/// Queues a request's reply plus what protocol says must follow it.
async fn reply_to(
    reply: Message,
    host: &HostState,
    state: &ClientState,
    outbound: &Outbound,
) -> Result<(), String> {
    let handshake = matches!(reply, Message::HelloAck { .. });
    let attached_pane = match &reply {
        Message::Snapshot { pane_id, .. } => Some(*pane_id),
        _ => None,
    };
    outbound.send(&reply)?;
    if let Some(pane_id) = attached_pane {
        if let Some(status) = latest_status(&host.sessions, pane_id).await {
            outbound.send(&status)?;
        }
    }
    // Protocol §3.2: the session list follows the ack.
    if handshake {
        outbound.send(&handshake::sessions(host).await)?;
        if state.status_all {
            for status in pending_statuses(&host.sessions).await {
                outbound.send(&status)?;
            }
        }
    }
    Ok(())
}

/// Queues one host event, if this connection follows what it is about.
async fn forward(
    event: Event,
    host: &HostState,
    state: &ClientState,
    outbound: &Outbound,
) -> Result<(), String> {
    match event {
        // Follow the session even after its last watched pane is gone, so closing the
        // active pane still redraws the tree.
        Event::Layout { session_id, frame } | Event::SizeState { session_id, frame } => {
            if state.session == Some(session_id) {
                outbound.control(frame)?;
            }
        }
        Event::Exited { pane_id, code } => {
            if state.watched.contains_key(&pane_id) {
                outbound.send(&Message::PaneExited { pane_id, code })?;
            }
        }
        Event::Status { pane_id, frame } => {
            if state.status_all || state.watched.contains_key(&pane_id) {
                outbound.control(frame)?;
            }
        }
        Event::SessionsChanged => {
            // Like the original `dispatch(Sessions)`: before `hello` a client gets an error.
            outbound.send(&sessions_or_error(host, state).await)?;
        }
        Event::Resized { pane_id } => {
            if state.watched.contains_key(&pane_id) && !state.scrolled_back(pane_id) {
                outbound.snapshot(pane_id);
            }
        }
        Event::Delta { pane_id, frame } => {
            let Some(delta_ok) = state.watched.get(&pane_id).copied() else {
                return Ok(());
            };
            // A scrolled-back window is reading history: live repaints would
            // tear the view it is looking at (protocol §3.3).
            if state.scrolled_back(pane_id) {
                return Ok(());
            }
            if delta_ok {
                outbound.delta(pane_id, frame)?;
            } else {
                // Taken when the writer gets there, so a burst costs one snapshot.
                outbound.snapshot(pane_id);
            }
        }
    }
    Ok(())
}

async fn sessions_or_error(host: &HostState, state: &ClientState) -> Message {
    if state.greeted {
        handshake::sessions(host).await
    } else {
        Message::Error {
            code: "protocol".into(),
            detail: "hello must be the first message".into(),
            id: None,
        }
    }
}

/// The `t` of a payload this build cannot decode, for the `unsupported` reply.
fn type_tag(payload: &[u8]) -> Option<String> {
    let value: ciborium::value::Value = ciborium::from_reader(payload).ok()?;
    let ciborium::value::Value::Map(entries) = value else {
        return None;
    };
    entries
        .into_iter()
        .find_map(|(key, value)| match (key, value) {
            (ciborium::value::Value::Text(name), ciborium::value::Value::Text(tag))
                if name == "t" =>
            {
                Some(tag)
            }
            _ => None,
        })
}
