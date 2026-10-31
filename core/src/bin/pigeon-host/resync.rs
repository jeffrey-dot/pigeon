//! What a connection gets after it fell behind the event bus.
//!
//! Events reach a client task through a bounded `broadcast` channel. The task only queues
//! them (see `outbound`), so it falls behind only when it is starved itself; then it loses
//! the oldest events. Deltas can be lost safely because a snapshot replaces them, but a
//! lost `layout`, `status` or `pane_exited` would leave the window wrong until something
//! else changes. So instead of replaying, the host re-queues the current state of
//! everything this connection follows.

use pigeon_core::proto::Message;

use crate::client_state::ClientState;
use crate::handshake;
use crate::host_state::HostState;
use crate::outbound::Outbound;
use crate::pane_query::current_status;

pub async fn resync(
    host: &HostState,
    state: &ClientState,
    outbound: &Outbound,
) -> Result<(), String> {
    if !state.greeted {
        // Nothing was negotiated yet, so nothing was sent that could be stale.
        return Ok(());
    }
    // Titles and the session set first: the client places panes by this list.
    outbound.send(&handshake::sessions(host).await)?;

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
        outbound.send(&layout)?;
    }
    for size in &sizes {
        outbound.send(size)?;
    }
    // `idle` included: a lost clear must replace what the window last showed.
    for &pane_id in &panes {
        if let Some(status) = current_status(&host.sessions, pane_id).await {
            outbound.send(&status)?;
        }
    }

    let mut watched: Vec<u32> = state.watched.keys().copied().collect();
    watched.sort_unstable();
    for pane_id in watched {
        if let Some(code) = host.exit_code(pane_id) {
            outbound.send(&Message::PaneExited { pane_id, code })?;
            continue;
        }
        // A scrolled-back window is reading history and gets the live screen when it
        // scrolls back to 0; everyone else gets a snapshot, which resets the delta base.
        if state.scrolled_back(pane_id) {
            continue;
        }
        outbound.snapshot(pane_id);
    }
    Ok(())
}
