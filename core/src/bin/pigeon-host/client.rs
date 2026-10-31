//! One connection: decode frames into `dispatch`, and queue the host's events this
//! connection cares about for its writer (`outbound`).

use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;

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

    loop {
        tokio::select! {
            read = reader.read_buf(&mut inbox) => {
                let count = read.map_err(|error| error.to_string())?;
                if count == 0 {
                    return Ok(());
                }
                if !drain_inbox(&mut inbox, host, &mut state, outbound).await? {
                    return Ok(());
                }
            }
            event = subscribed.recv() => {
                match event {
                    Ok(event) => forward(event, host, &state, outbound).await?,
                    Err(RecvError::Lagged(missed)) => {
                        if std::env::var_os("PIGEON_DEBUG").is_some() {
                            eprintln!("pigeon-host: client {client_id} lagged by {missed} events");
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
        match proto::decode_frame(inbox) {
            Ok(Some((message, used))) => {
                inbox.drain(..used);
                if std::env::var_os("PIGEON_DEBUG").is_some() {
                    eprintln!("pigeon-host: <- {}", message.name());
                }
                if let Some(reply) = dispatch(message, host, state).await {
                    reply_to(reply, host, state, outbound).await?;
                }
                // Outside the reply branch: a replay has no single reply.
                for frame in std::mem::take(&mut state.outbox) {
                    outbound.send(&frame)?;
                }
            }
            Ok(None) => return Ok(true),
            Err(PgError::Unsupported) => {
                let t_of = type_tag(inbox).unwrap_or_default();
                outbound.send(&Message::Unsupported { t_of })?;
                inbox.clear();
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

/// The `t` of a frame this build cannot decode, for the `unsupported` reply.
fn type_tag(buf: &[u8]) -> Option<String> {
    let payload = buf.get(4..)?;
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
