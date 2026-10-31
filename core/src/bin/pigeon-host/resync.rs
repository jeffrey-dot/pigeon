//! What a connection gets after it fell behind the event bus.
//!
//! Events reach a client through a bounded `broadcast` channel; a client that reads too
//! slowly (a stalled ssh link, a window busy elsewhere) loses the oldest ones. Deltas can
//! be lost safely because a snapshot replaces them, but a lost `layout`, `status` or
//! `pane_exited` would leave the window wrong until something else changes. So instead of
//! replaying, the host re-sends the current state of everything this connection follows.

use tokio::io::{AsyncWrite, WriteHalf};

use pigeon_core::proto::Message;

use crate::client::send;
use crate::client_state::ClientState;
use crate::handshake;
use crate::host_state::HostState;
use crate::pane_query::{current_status, snapshot_of};

pub async fn resync<S>(
    host: &HostState,
    state: &ClientState,
    writer: &mut WriteHalf<S>,
) -> Result<(), String>
where
    S: AsyncWrite + Unpin,
{
    if !state.greeted {
        // Nothing was negotiated yet, so nothing was sent that could be stale.
        return Ok(());
    }
    // Titles and the session set first: the client places panes by this list.
    send(writer, &handshake::sessions(host).await).await?;

    let (layout, sizes, panes) = {
        let map = host.sessions.lock().await;
        let followed = state
            .session
            .and_then(|id| map.get(&id).map(|session| (id, session)));
        let layout = followed.map(|(id, session)| session.layout_message(id));
        let sizes = followed
            .map(|(_, session)| session.size_states())
            .unwrap_or_default();
        let mut panes: Vec<u32> = if state.status_all {
            map.values().flat_map(|s| s.panes.keys().copied()).collect()
        } else {
            state
                .watched
                .keys()
                .copied()
                .filter(|pane| map.values().any(|s| s.panes.contains_key(pane)))
                .collect()
        };
        panes.sort_unstable();
        (layout, sizes, panes)
    };
    // A followed session that is gone is not re-announced: the session list above already
    // says it is gone, and `state.session` keeps pointing at it long after the close.
    if let Some(layout) = layout {
        send(writer, &layout).await?;
    }
    for size in &sizes {
        send(writer, size).await?;
    }
    // `idle` included: a lost clear must replace what the window last showed.
    for &pane_id in &panes {
        if let Some(status) = current_status(&host.sessions, pane_id).await {
            send(writer, &status).await?;
        }
    }

    let mut watched: Vec<u32> = state.watched.keys().copied().collect();
    watched.sort_unstable();
    for pane_id in watched {
        if let Some(code) = host.exit_code(pane_id) {
            send(writer, &Message::PaneExited { pane_id, code }).await?;
            continue;
        }
        // A scrolled-back window is reading history and gets the live screen when it
        // scrolls back to 0; everyone else gets a snapshot, which resets the delta base.
        if state.scrolled_back(pane_id) {
            continue;
        }
        if let Some(snapshot) = snapshot_of(&host.sessions, pane_id, false).await {
            send(writer, &snapshot).await?;
        }
    }
    Ok(())
}
