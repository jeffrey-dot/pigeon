//! Requests about one pane's life and I/O: creating a session, attaching, scrolling and
//! forwarding input.

use pigeon_core::proto::Message;
use pigeon_core::session::SizePolicy;

use crate::client_state::ClientState;
use crate::cmd::Cmd;
use crate::dispatch::no_such_pane;
use crate::event::{encode, Event};
use crate::host_state::{broadcast_size, session_of, HostState, Session};
use crate::pane_actor::spawn_pane;
use crate::pane_query::{catch_up, latest_status, scroll as scroll_pane, sender_of, snapshot_of};

const DEFAULT_COLS: u16 = 80;
const DEFAULT_ROWS: u16 = 24;

/// Forwards a command to a pane, waiting for queue space: input must arrive in order.
pub async fn send(host: &HostState, pane_id: u32, command: Cmd) {
    if let Some(sender) = sender_of(&host.sessions, pane_id).await {
        let _ = sender.send(command).await;
    }
}

pub async fn new_session(
    host: &HostState,
    id: u32,
    cwd: Option<String>,
    requested: Option<String>,
) -> Message {
    let shell = requested.unwrap_or_else(|| host.shell.clone());
    let launch_cwd = cwd.clone().unwrap_or_default();
    let Some((pane_id, sender)) = spawn_pane(host, &shell, cwd, DEFAULT_COLS, DEFAULT_ROWS) else {
        return Message::Error {
            code: "internal".into(),
            detail: "cannot spawn the shell".into(),
            id: Some(id),
        };
    };
    let session = Session::new(
        pane_id,
        sender,
        &shell,
        launch_cwd,
        DEFAULT_COLS,
        DEFAULT_ROWS,
    );
    host.sessions.lock().await.insert(pane_id, session);
    Message::Created {
        id,
        session_id: pane_id,
        pane_id,
    }
}

pub async fn attach(
    host: &HostState,
    state: &mut ClientState,
    id: u32,
    pane_id: u32,
    size_policy: Option<String>,
    history: Option<bool>,
    since: Option<u32>,
) -> Option<Message> {
    let sessions = &host.sessions;
    let session_id = {
        let map = sessions.lock().await;
        session_of(&map, pane_id)
    };
    let Some(session_id) = session_id else {
        return Some(no_such_pane(pane_id, Some(id)));
    };
    // A reconnect (issue #4): replay what the client missed, which is usually a
    // few rows instead of the whole screen. Only for a client that takes deltas.
    let reply = match since.filter(|_| state.delta_ok) {
        Some(since) => catch_up(sessions, pane_id, since).await.map(|frames| {
            state.outbox.extend(frames);
            None
        }),
        None => snapshot_of(sessions, pane_id, history.unwrap_or(true))
            .await
            .map(Some),
    };
    let Some(snapshot) = reply else {
        return Some(no_such_pane(pane_id, Some(id)));
    };
    state.watched.insert(pane_id, state.delta_ok);
    state.session = Some(session_id);
    // Protocol §6: the first `attach` decides the policy; later ones merely join.
    let mut map = sessions.lock().await;
    if let Some(session) = map.get_mut(&session_id) {
        let policy = SizePolicy::from_name(size_policy.as_deref());
        let current = session.size();
        session.vote.join(state.client_id, policy, current);
        // The tree follows the snapshot, so a reconnecting window redraws
        // the same split geometry and learns who owns the size.
        if let Some(frame) = encode(&session.layout_message(session_id)) {
            let _ = host.events.send(Event::Layout { session_id, frame });
        }
        broadcast_size(session_id, session, &host.events);
    }
    // `latest_status` takes the sessions lock again.
    drop(map);
    if snapshot.is_none() {
        // The caught-up client still needs the status it may have missed.
        if let Some(status) = latest_status(sessions, pane_id).await {
            state.outbox.push(status);
        }
    }
    snapshot
}

/// Protocol §3.3: `offset` moves this window's view relative to where it is; 0 returns to
/// the live screen.
pub async fn scroll(
    host: &HostState,
    state: &mut ClientState,
    pane_id: u32,
    offset: i32,
) -> Option<Message> {
    let current = state.views.get(&pane_id).copied().unwrap_or(0);
    let wanted = if offset == 0 {
        0
    } else {
        (current as i64 - i64::from(offset)).max(0) as usize
    };
    let Some(sender) = sender_of(&host.sessions, pane_id).await else {
        return Some(no_such_pane(pane_id, None));
    };
    let (message, applied) = scroll_pane(&sender, wanted).await?;
    state.views.insert(pane_id, applied);
    Some(message)
}
