//! Request/reply round trips to a pane's task. Each takes the sessions lock only long
//! enough to find the pane's channel, never across the await on the reply.

use tokio::sync::{mpsc, oneshot};

use pigeon_core::proto::Message;

use crate::cmd::Cmd;
use crate::host_state::Sessions;

pub async fn sender_of(sessions: &Sessions, pane_id: u32) -> Option<mpsc::Sender<Cmd>> {
    let map = sessions.lock().await;
    map.values()
        .find_map(|session| session.panes.get(&pane_id).cloned())
}

/// What a reconnecting client missed since `since`: deltas, or one snapshot.
pub async fn catch_up(sessions: &Sessions, pane_id: u32, since: u32) -> Option<Vec<Message>> {
    let sender = sender_of(sessions, pane_id).await?;
    let (tx, rx) = oneshot::channel();
    sender.send(Cmd::Since { since, reply: tx }).await.ok()?;
    rx.await.ok()
}

pub async fn snapshot_of(sessions: &Sessions, pane_id: u32, history: bool) -> Option<Message> {
    let sender = sender_of(sessions, pane_id).await?;
    let (tx, rx) = oneshot::channel();
    sender
        .send(Cmd::Snapshot { history, reply: tx })
        .await
        .ok()?;
    rx.await.ok()
}

/// The pane's current status, so a client that attaches later still sees it.
pub async fn latest_status(sessions: &Sessions, pane_id: u32) -> Option<Message> {
    let sender = sender_of(sessions, pane_id).await?;
    let (tx, rx) = oneshot::channel();
    sender.send(Cmd::LatestStatus { reply: tx }).await.ok()?;
    rx.await.ok().flatten()
}

/// Every pane's pending status, for a `status_all` client that just connected; the session
/// list goes first so the client can place each pane.
pub async fn pending_statuses(sessions: &Sessions) -> Vec<Message> {
    let panes: Vec<u32> = {
        let map = sessions.lock().await;
        let mut panes: Vec<u32> = map.values().flat_map(|s| s.panes.keys().copied()).collect();
        panes.sort_unstable();
        panes
    };
    let mut out = Vec::new();
    for pane_id in panes {
        if let Some(status) = latest_status(sessions, pane_id).await {
            out.push(status);
        }
    }
    out
}

/// Asks a pane to look `offset` lines back; returns the view and the offset it applied.
pub async fn scroll(sender: &mpsc::Sender<Cmd>, offset: usize) -> Option<(Message, usize)> {
    let (tx, rx) = oneshot::channel();
    sender.send(Cmd::Scroll { offset, reply: tx }).await.ok()?;
    rx.await.ok()
}
