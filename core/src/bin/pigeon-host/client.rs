//! One connection: decode frames into `dispatch`, and forward the host's events this
//! connection cares about.

use std::sync::atomic::{AtomicU32, Ordering};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, WriteHalf};
use tokio::sync::broadcast::error::RecvError;

use pigeon_core::error::PgError;
use pigeon_core::proto::{self, Message};

use crate::client_state::ClientState;
use crate::dispatch::dispatch;
use crate::event::Event;
use crate::handshake;
use crate::host_state::HostState;
use crate::pane_query::{latest_status, pending_statuses, snapshot_of};
use crate::resync::resync;

/// Connection ids the host hands out in `hello_ack` (protocol §3.1).
static NEXT_CLIENT: AtomicU32 = AtomicU32::new(1);

pub async fn handle_client<S>(stream: S, host: HostState) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let client_id = NEXT_CLIENT.fetch_add(1, Ordering::Relaxed);
    let result = client_loop(stream, &host, client_id).await;
    // A closed window must stop counting towards the session's size policy, or
    // `smallest` would stay pinned to a box nobody can see any more.
    host.forget_client(client_id).await;
    result
}

async fn client_loop<S>(stream: S, host: &HostState, client_id: u32) -> Result<(), String>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (mut reader, mut writer) = tokio::io::split(stream);
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
                if !drain_inbox(&mut inbox, host, &mut state, &mut writer).await? {
                    return Ok(());
                }
            }
            event = subscribed.recv() => {
                match event {
                    Ok(event) => forward(event, host, &state, &mut writer).await?,
                    Err(RecvError::Lagged(missed)) => {
                        if std::env::var_os("PIGEON_DEBUG").is_some() {
                            eprintln!("pigeon-host: client {client_id} lagged by {missed} events");
                        }
                        // What is still queued predates the state about to be sent: skip it,
                        // so nothing older than the resync reaches the window after it.
                        subscribed = subscribed.resubscribe();
                        resync(host, &state, &mut writer).await?;
                    }
                    // The host holds a sender for as long as it runs.
                    Err(RecvError::Closed) => return Ok(()),
                }
            }
        }
    }
}

/// Handles every whole frame in `inbox`. Returns `false` when the connection must close
/// (a frame that does not decode at all).
async fn drain_inbox<S>(
    inbox: &mut Vec<u8>,
    host: &HostState,
    state: &mut ClientState,
    writer: &mut WriteHalf<S>,
) -> Result<bool, String>
where
    S: AsyncWrite + Unpin,
{
    loop {
        match proto::decode_frame(inbox) {
            Ok(Some((message, used))) => {
                inbox.drain(..used);
                if std::env::var_os("PIGEON_DEBUG").is_some() {
                    eprintln!("pigeon-host: <- {}", message.name());
                }
                if let Some(reply) = dispatch(message, host, state).await {
                    reply_to(reply, host, state, writer).await?;
                }
                // Outside the reply branch: a replay has no single reply.
                for frame in std::mem::take(&mut state.outbox) {
                    send(writer, &frame).await?;
                }
            }
            Ok(None) => return Ok(true),
            Err(PgError::Unsupported) => {
                let t_of = type_tag(inbox).unwrap_or_default();
                send(writer, &Message::Unsupported { t_of }).await?;
                inbox.clear();
            }
            Err(error) => {
                let detail = format!("{error}");
                let message = Message::Error {
                    code: "protocol".into(),
                    detail,
                    id: None,
                };
                send(writer, &message).await?;
                return Ok(false);
            }
        }
    }
}

/// Sends a request's reply plus what protocol says must follow it.
async fn reply_to<S>(
    reply: Message,
    host: &HostState,
    state: &ClientState,
    writer: &mut WriteHalf<S>,
) -> Result<(), String>
where
    S: AsyncWrite + Unpin,
{
    let handshake = matches!(reply, Message::HelloAck { .. });
    let attached_pane = match &reply {
        Message::Snapshot { pane_id, .. } => Some(*pane_id),
        _ => None,
    };
    send(writer, &reply).await?;
    if let Some(pane_id) = attached_pane {
        if let Some(status) = latest_status(&host.sessions, pane_id).await {
            send(writer, &status).await?;
        }
    }
    // Protocol §3.2: the session list follows the ack.
    if handshake {
        send(writer, &handshake::sessions(host).await).await?;
        if state.status_all {
            for status in pending_statuses(&host.sessions).await {
                send(writer, &status).await?;
            }
        }
    }
    Ok(())
}

/// Passes one host event on, if this connection follows what it is about.
async fn forward<S>(
    event: Event,
    host: &HostState,
    state: &ClientState,
    writer: &mut WriteHalf<S>,
) -> Result<(), String>
where
    S: AsyncWrite + Unpin,
{
    match event {
        // Follow the session even after its last watched pane is gone, so closing the
        // active pane still redraws the tree.
        Event::Layout {
            session_id,
            message,
        }
        | Event::SizeState {
            session_id,
            message,
        } => {
            if state.session == Some(session_id) {
                send(writer, &message).await?;
            }
        }
        Event::Exited { pane_id, code } => {
            if state.watched.contains_key(&pane_id) {
                send(writer, &Message::PaneExited { pane_id, code }).await?;
            }
        }
        Event::Status { pane_id, message } => {
            if state.status_all || state.watched.contains_key(&pane_id) {
                send(writer, &message).await?;
            }
        }
        Event::SessionsChanged => {
            // Like the original `dispatch(Sessions)`: before `hello` a client gets an error.
            send(writer, &sessions_or_error(host, state).await).await?;
        }
        Event::Resized { pane_id } => {
            if !state.watched.contains_key(&pane_id) || state.scrolled_back(pane_id) {
                return Ok(());
            }
            if let Some(snapshot) = snapshot_of(&host.sessions, pane_id, false).await {
                send(writer, &snapshot).await?;
            }
        }
        Event::Delta { pane_id, message } => {
            let Some(delta_ok) = state.watched.get(&pane_id).copied() else {
                return Ok(());
            };
            // A scrolled-back window is reading history: live repaints would
            // tear the view it is looking at (protocol §3.3).
            if state.scrolled_back(pane_id) {
                return Ok(());
            }
            if delta_ok {
                send(writer, &message).await?;
            } else if let Some(snapshot) = snapshot_of(&host.sessions, pane_id, false).await {
                send(writer, &snapshot).await?;
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

pub async fn send<S>(writer: &mut WriteHalf<S>, message: &Message) -> Result<(), String>
where
    S: AsyncWrite + Unpin,
{
    let frame = proto::encode_frame(message).map_err(|error| format!("encode: {error}"))?;
    writer
        .write_all(&frame)
        .await
        .map_err(|error| error.to_string())?;
    // tokio's stdout is buffered; without this a `--stdio` client would wait for a frame
    // that is still sitting in the writer.
    writer.flush().await.map_err(|error| error.to_string())
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
