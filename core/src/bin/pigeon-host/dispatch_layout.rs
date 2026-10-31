//! Requests that change a session's tree or box: split, resize, focus, zoom, close.
//! Each one ends in `apply_layout` (or `broadcast_size`), so every window redraws.

use pigeon_core::layout::Axis;
use pigeon_core::proto::{Message, SplitAxis};
use pigeon_core::session::empty_layout_message;

use crate::cmd::{send_or_queue, Cmd};
use crate::dispatch::no_such_pane;
use crate::event::Event;
use crate::host_state::{apply_layout, broadcast_size, session_of, HostState};
use crate::pane_actor::spawn_pane;

pub async fn split(
    host: &HostState,
    id: u32,
    pane_id: u32,
    axis: SplitAxis,
    ratio: f32,
) -> Option<Message> {
    let axis = match axis {
        SplitAxis::Vertical => Axis::Vertical,
        SplitAxis::Horizontal => Axis::Horizontal,
    };
    let mut map = host.sessions.lock().await;
    let Some(session_id) = session_of(&map, pane_id) else {
        return Some(no_such_pane(pane_id, Some(id)));
    };
    let session = map.get_mut(&session_id)?;
    let (cols, rows) = session.split_size(axis);
    let Some((new_pane, sender)) = spawn_pane(host, &host.shell, None, cols, rows) else {
        return Some(Message::Error {
            code: "internal".into(),
            detail: "cannot spawn the shell".into(),
            id: Some(id),
        });
    };
    let (box_cols, box_rows) = session.size();
    if session
        .layout
        .split(pane_id, new_pane, axis, ratio, box_cols, box_rows)
        .is_err()
    {
        return Some(no_such_pane(pane_id, Some(id)));
    }
    session.panes.insert(new_pane, sender);
    // The new pane takes focus, like every terminal's split.
    session.layout.focus(new_pane);
    apply_layout(session_id, session, &host.events);
    None
}

pub async fn resize_split(host: &HostState, id: u32, split_id: u32, ratio: f32) -> Option<Message> {
    let bad_split = || Message::Error {
        code: "bad_request".into(),
        detail: format!("split {split_id}"),
        id: Some(id),
    };
    let mut map = host.sessions.lock().await;
    let Some((session_id, session)) = map
        .iter_mut()
        .find(|(_, session)| session.layout.split_of_any(split_id))
    else {
        return Some(bad_split());
    };
    let (cols, rows) = session.size();
    if session
        .layout
        .set_ratio(split_id, ratio, cols, rows)
        .is_err()
    {
        return Some(bad_split());
    }
    apply_layout(*session_id, session, &host.events);
    None
}

pub async fn focus(host: &HostState, client_id: u32, pane_id: u32) -> Option<Message> {
    let mut map = host.sessions.lock().await;
    let session_id = session_of(&map, pane_id)?;
    let session = map.get_mut(&session_id)?;
    session.layout.focus(pane_id);
    // `latest`: focusing is what makes a window the one that owns the size.
    if session.vote.focus(client_id) {
        session.refit();
    }
    apply_layout(session_id, session, &host.events);
    None
}

pub async fn resize_pane(
    host: &HostState,
    client_id: u32,
    pane_id: u32,
    cols: u16,
    rows: u16,
) -> Option<Message> {
    let mut map = host.sessions.lock().await;
    let session_id = session_of(&map, pane_id)?;
    let session = map.get_mut(&session_id)?;
    // The box this client's grid implies for the whole session.
    let wanted = session.box_for(pane_id, cols, rows)?;
    session.vote.report(client_id, wanted);
    if session.refit() {
        apply_layout(session_id, session, &host.events);
    } else {
        // The request stands or was overruled either way: say which (protocol §3.2).
        broadcast_size(session_id, session, &host.events);
    }
    None
}

/// `zoom_pane` with `on`, `unzoom` without.
pub async fn zoom(host: &HostState, id: u32, pane_id: u32, on: bool) -> Option<Message> {
    let mut map = host.sessions.lock().await;
    let Some(session_id) = session_of(&map, pane_id) else {
        return Some(no_such_pane(pane_id, Some(id)));
    };
    let session = map.get_mut(&session_id)?;
    let changed = if on {
        session.layout.zoom(pane_id)
    } else {
        session.layout.unzoom(pane_id)
    };
    if !changed {
        return Some(no_such_pane(pane_id, Some(id)));
    }
    apply_layout(session_id, session, &host.events);
    None
}

pub async fn close(host: &HostState, pane_id: u32) -> Option<Message> {
    let mut map = host.sessions.lock().await;
    let session_id = session_of(&map, pane_id)?;
    let session = map.get_mut(&session_id)?;
    if let Some(sender) = session.panes.remove(&pane_id) {
        send_or_queue(&sender, Cmd::Kill);
    }
    if let Ok(mut exits) = host.exits.lock() {
        exits.remove(&pane_id);
    }
    let _ = session.layout.close(pane_id);
    if session.layout.is_empty() {
        let _ = host.events.send(Event::Layout {
            session_id,
            message: empty_layout_message(session_id),
        });
        map.remove(&session_id);
    } else {
        apply_layout(session_id, session, &host.events);
    }
    None
}
