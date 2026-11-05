use super::*;
use pigeon_core::api::{Node, Scroll, WireCell, WireCursor};

fn session(id: u32, panes: &[u32]) -> SessionInfo {
    SessionInfo {
        session_id: id,
        title: format!("Session {id}"),
        cwd: "/project".into(),
        panes: panes.to_vec(),
        active_pane: panes.first().copied(),
        active_title: None,
        active_program: None,
    }
}
fn list(model: &mut SessionModel) -> Vec<Message> {
    let effects = model.apply(&Message::Sessions {
        items: vec![session(1, &[1]), session(2, &[2])],
    });
    barrier(model);
    effects
}
fn barrier(model: &mut SessionModel) {
    if let Some(ts) = model.barrier {
        model.apply(&Message::Pong { ts });
    }
}

fn layout(session_id: u32, pane: u32) -> Message {
    Message::Layout {
        session_id,
        tree: Node::Leaf { leaf: pane },
        zoom: None,
        focus: Some(pane),
    }
}
fn snapshot(pane: u32) -> Message {
    Message::Snapshot {
        pane_id: pane,
        seq: 0,
        cols: 1,
        rows: 1,
        alt: false,
        cursor: WireCursor {
            x: 0,
            y: 0,
            visible: true,
            shape: 0,
        },
        scroll: Scroll { top: 0, bottom: 0 },
        screen: vec![vec![WireCell {
            codepoint: 65,
            attrs: 0,
            fg: 7,
            bg: 0,
            link: None,
        }]],
        text: vec![],
        history: None,
        links: None,
        history_len: None,
        offset: None,
        modes: None,
    }
}
fn model() -> SessionModel {
    SessionModel::new(true)
}
#[test]
fn selecting_cached_session_reestablishes_host_layout_subscription() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.select(2);
    barrier(&mut model);
    model.apply(&snapshot(2));
    model.apply(&layout(2, 2));
    let effects = model.select(1);
    assert!(effects
        .iter()
        .any(|m| matches!(m, Message::Attach { pane_id: 1, .. })));
    assert_eq!(model.input_pane(), None);
    model.apply(&layout(2, 2));
    assert!(model.layout.is_empty());
    barrier(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    assert_eq!(model.input_pane(), Some(1));
    assert!(!effects
        .iter()
        .any(|m| matches!(m, Message::ClosePane { .. })));
}
#[test]
fn screen_before_layout_is_cached_but_cannot_receive_input() {
    let mut model = model();
    list(&mut model);
    model.select(2);
    barrier(&mut model);
    model.apply(&snapshot(2));
    assert!(model.panes.contains_key(&2));
    assert_eq!(model.input_pane(), None);
    model.apply(&layout(2, 2));
    assert_eq!(model.input_pane(), Some(2));
}
#[test]
fn background_delta_gap_never_changes_layout_subscription() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.select(2);
    barrier(&mut model);
    let effects = model.apply(&Message::Delta {
        pane_id: 1,
        seq: 99,
        rows: vec![],
        text: vec![],
        cursor: None,
        scroll: None,
        links: None,
        alt: None,
        modes: None,
    });
    assert!(effects.is_empty());
    assert_eq!(model.selected, Some(2));
}
#[test]
fn closed_pane_late_screen_cannot_resurrect_input() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.apply(&Message::Sessions {
        items: vec![session(2, &[2])],
    });
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    assert!(!model.panes.contains_key(&1));
    assert_eq!(model.input_pane(), None);
}
#[test]
fn switching_requests_live_and_disconnect_disables_input() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.select(2);
    barrier(&mut model);
    let effects = model.select(1);
    assert!(effects.iter().any(|m| matches!(
        m,
        Message::Scroll {
            pane_id: 1,
            offset: 0
        }
    )));
    model.apply(&layout(1, 1));
    model.disconnect();
    assert_eq!(model.input_pane(), None);
}

#[test]
fn every_selection_change_invalidates_record_inspection_even_when_returning() {
    let mut model = model();
    list(&mut model);
    let generation = model.selection_generation;
    model.select(2);
    barrier(&mut model);
    model.select(1);
    assert_ne!(model.selection_generation, generation);
}

#[test]
fn switch_barrier_rejects_queued_layout_and_screen_and_needs_new_screen() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.select(2);
    model.select(1);
    model.apply(&layout(1, 1));
    model.apply(&snapshot(1));
    assert!(model.layout.is_empty());
    barrier(&mut model);
    model.apply(&layout(1, 1));
    assert_eq!(model.input_pane(), None);
    model.apply(&snapshot(1));
    assert_eq!(model.input_pane(), Some(1));
}

#[test]
fn exited_pane_can_still_be_copied_scrolled_or_closed_but_never_typed_into() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.apply(&Message::PaneExited {
        pane_id: 1,
        code: 0,
    });
    assert_eq!(model.input_pane(), None);
    assert_eq!(model.focused_pane(), Some(1));
    assert!(matches!(
        model.close_selected()[0],
        Message::ClosePane { pane_id: 1, .. }
    ));
}

#[test]
fn switching_drops_old_pending_scroll_replies_before_requesting_live() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.panes.get_mut(&1).unwrap().scroll.request(-20);
    model.select(2);
    model.select(1);
    barrier(&mut model);
    model.apply(&snapshot(1));
    assert!(!model.panes[&1].scroll.active());
}

#[test]
fn malformed_snapshot_cannot_enable_cached_pane_input_after_switch() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.select(2);
    model.select(1);
    barrier(&mut model);
    model.apply(&layout(1, 1));
    let mut bad = snapshot(1);
    if let Message::Snapshot { screen, .. } = &mut bad {
        screen.clear();
    }
    model.apply(&bad);
    assert_eq!(model.input_pane(), None);
}

#[test]
fn ignored_scroll_reply_cannot_certify_cached_history_as_current_screen() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.select(2);
    model.select(1);
    barrier(&mut model);
    model.apply(&layout(1, 1));
    let mut unsolicited = snapshot(1);
    if let Message::Snapshot {
        history_len,
        offset,
        ..
    } = &mut unsolicited
    {
        *history_len = Some(30);
        *offset = Some(0);
    }
    model.apply(&unsolicited);
    assert_eq!(model.input_pane(), None);
    model.apply(&snapshot(1));
    assert_eq!(model.input_pane(), Some(1));
}

#[test]
fn reconnect_restores_selected_session_only_after_new_screen_and_layout() {
    let mut model = model();
    list(&mut model);
    model.select(2);
    barrier(&mut model);
    model.apply(&snapshot(2));
    model.apply(&layout(2, 2));
    let old_generation = model.selection_generation;
    model.disconnect();
    model.reconnect(true, true);
    assert_eq!(model.input_pane(), None);
    let effects = model.apply(&Message::Sessions {
        items: vec![session(1, &[1]), session(2, &[2])],
    });
    assert_eq!(model.selected, Some(2));
    assert!(effects
        .iter()
        .any(|m| matches!(m, Message::Attach { pane_id: 2, .. })));
    assert!(model.selection_generation > old_generation);
    barrier(&mut model);
    model.apply(&layout(2, 2));
    assert_eq!(model.input_pane(), None);
    model.apply(&snapshot(2));
    assert_eq!(model.input_pane(), Some(2));
}

#[test]
fn restarted_host_never_reuses_old_pane_or_automatically_creates_a_session() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.disconnect();
    model.reconnect(false, true);
    assert!(model.panes.is_empty());
    assert_eq!(model.selected, None);
    let effects = model.apply(&Message::Sessions { items: vec![] });
    assert!(effects.is_empty());
    assert_eq!(model.input_pane(), None);
}
