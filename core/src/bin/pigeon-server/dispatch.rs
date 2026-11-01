//! One inbound message → its effect and at most one direct reply. Session-tree edits live
//! in `dispatch_layout`, pane lifecycle and I/O in `dispatch_pane`.

use pigeon_core::proto::Message;
use pigeon_core::term::Palette;

use crate::client_state::ClientState;
use crate::cmd::Cmd;
use crate::host_state::HostState;
use crate::{dispatch_layout as layout, dispatch_pane as pane, handshake};

pub async fn dispatch(
    message: Message,
    host: &HostState,
    state: &mut ClientState,
) -> Option<Message> {
    if !state.greeted && !matches!(message, Message::Hello { .. }) {
        return Some(Message::Error {
            code: "protocol".into(),
            detail: "hello must be the first message".into(),
            id: None,
        });
    }
    match message {
        Message::Hello {
            proto, features, ..
        } => Some(handshake::hello(proto, &features, state)),
        Message::Sessions { .. } => Some(handshake::sessions(host).await),
        Message::Appearance { fg, bg, ansi } => {
            appearance(host, fg, bg, &ansi).await;
            None
        }
        Message::NewSession {
            id,
            cwd,
            shell: requested,
            ..
        } => Some(pane::new_session(host, id, cwd, requested).await),
        Message::Attach {
            id,
            pane_id,
            size_policy,
            history,
            since,
        } => pane::attach(host, state, id, pane_id, size_policy, history, since).await,
        Message::SplitPane {
            id,
            pane_id,
            axis,
            ratio,
        } => layout::split(host, id, pane_id, axis, ratio).await,
        Message::ResizeSplit {
            id,
            split_id,
            ratio,
        } => layout::resize_split(host, id, split_id, ratio).await,
        Message::Focus { pane_id } => layout::focus(host, state.client_id, pane_id).await,
        Message::Keys { pane_id, bytes } => {
            pane::send(host, pane_id, Cmd::Input(bytes)).await;
            host.note_activity(state.client_id, pane_id).await;
            None
        }
        Message::Paste { pane_id, text } => {
            pane::send(host, pane_id, Cmd::Paste(text)).await;
            host.note_activity(state.client_id, pane_id).await;
            None
        }
        Message::Scroll { pane_id, offset } => pane::scroll(host, state, pane_id, offset).await,
        Message::ResizePane {
            pane_id,
            cols,
            rows,
        } => layout::resize_pane(host, state.client_id, pane_id, cols, rows).await,
        Message::ZoomPane { id, pane_id } => layout::zoom(host, id, pane_id, true).await,
        Message::Unzoom { id, pane_id } => layout::zoom(host, id, pane_id, false).await,
        Message::ClosePane { pane_id, .. } => layout::close(host, pane_id).await,
        Message::AckStatus { pane_id, seq } => {
            pane::send(host, pane_id, Cmd::Ack(seq)).await;
            None
        }
        Message::Ping { ts } => Some(Message::Pong { ts }),
        Message::Shutdown => {
            // `notify_one` keeps the permit if `serve` is between two accepts.
            host.stop.notify_one();
            None
        }
        // Protocol rule 2: a message we do not know is not an error.
        other => Some(Message::Unsupported {
            t_of: other.name().to_owned(),
        }),
    }
}

/// The client tells us what it is drawing, so the programs inside the panes can pick
/// their light or dark theme (harp answers the same queries from its theme).
async fn appearance(host: &HostState, fg: Option<u32>, bg: Option<u32>, ansi: &[u32]) {
    let Some(colours) = fg
        .zip(bg)
        .and_then(|(fg, bg)| Palette::from_rgb(fg, bg, ansi))
    else {
        // Not enough information (or a wrong-length palette): keep the old answer set
        // rather than inventing colours.
        return;
    };
    if let Ok(mut slot) = host.palette.lock() {
        *slot = Some(colours);
    }
    let map = host.sessions.lock().await;
    for session in map.values() {
        for sender in session.panes.values() {
            let _ = sender.try_send(Cmd::SetPalette(Some(colours)));
        }
    }
}

/// `{code: no_such_pane}` for a request about a pane this host does not have.
pub fn no_such_pane(pane_id: u32, id: Option<u32>) -> Message {
    Message::Error {
        code: "no_such_pane".into(),
        detail: format!("pane {pane_id}"),
        id,
    }
}
