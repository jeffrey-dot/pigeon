use super::*;

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
    model.panes.get_mut(&2).unwrap().scroll.request(-1);
    model.reconnect(true, true);
    model.disable_resume();
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

#[test]
fn quiet_resume_pong_allows_input_but_does_not_certify_a_new_screen_for_ack() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.disconnect();
    model.reconnect(true, true);
    let effects = model.apply(&Message::Sessions {
        items: vec![session(1, &[1])],
    });
    assert!(effects
        .iter()
        .any(|m| matches!(m, Message::Attach { since: Some(0), .. })));
    barrier(&mut model);
    model.apply(&layout(1, 1));
    assert_eq!(model.input_pane(), None);
    let ts = effects
        .iter()
        .filter_map(|m| {
            if let Message::Ping { ts } = m {
                Some(*ts)
            } else {
                None
            }
        })
        .next_back()
        .unwrap();
    model.apply(&Message::Pong { ts });
    assert_eq!(model.input_pane(), Some(1));
    assert!(!model.received.contains(&1));
    assert!(model
        .attach(1)
        .iter()
        .any(|m| matches!(m, Message::Attach { since: None, .. })));
}

#[test]
fn history_never_supplies_a_resume_sequence() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.panes.get_mut(&1).unwrap().scroll.request(-5);
    model.disconnect();
    model.reconnect(true, true);
    let effects = model.apply(&Message::Sessions {
        items: vec![session(1, &[1])],
    });
    assert!(effects
        .iter()
        .any(|m| matches!(m, Message::Attach { since: None, .. })));
}

#[test]
fn gap_before_restored_layout_invalidates_resume_pong_and_requests_full_picture() {
    let mut model = model();
    list(&mut model);
    model.apply(&snapshot(1));
    model.apply(&layout(1, 1));
    model.disconnect();
    model.reconnect(true, true);
    let effects = model.apply(&Message::Sessions {
        items: vec![session(1, &[1])],
    });
    barrier(&mut model);
    let ts = effects
        .iter()
        .rev()
        .find_map(|m| {
            if let Message::Ping { ts } = m {
                Some(*ts)
            } else {
                None
            }
        })
        .unwrap();
    let effects = model.apply(&Message::Delta {
        pane_id: 1,
        seq: 9,
        rows: vec![],
        text: vec![],
        cursor: None,
        scroll: None,
        links: None,
        alt: None,
        modes: None,
    });
    assert!(effects
        .iter()
        .any(|m| matches!(m, Message::Attach { since: None, .. })));
    model.apply(&Message::Pong { ts });
    model.apply(&layout(1, 1));
    assert_eq!(model.input_pane(), None);
}
