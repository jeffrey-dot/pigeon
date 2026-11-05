use super::*;

#[test]
fn unmodified_arrows_follow_decckm() {
    let none = ModifierType::empty();
    assert_eq!(encode(Key::Up, none, false), Some(b"\x1b[A".to_vec()));
    assert_eq!(encode(Key::Up, none, true), Some(b"\x1bOA".to_vec()));
}

#[test]
fn ctrl_arrow_carries_the_modifier() {
    let ctrl = ModifierType::CONTROL_MASK;
    assert_eq!(encode(Key::Right, ctrl, false), Some(b"\x1b[1;5C".to_vec()));
}

#[test]
fn ctrl_c_is_etx() {
    let ctrl = ModifierType::CONTROL_MASK;
    assert_eq!(encode(Key::c, ctrl, false), Some(vec![0x03]));
}

#[test]
fn alt_prefixes_escape() {
    let alt = ModifierType::ALT_MASK;
    assert_eq!(encode(Key::b, alt, false), Some(vec![0x1b, b'b']));
}

#[test]
fn bare_modifiers_produce_nothing() {
    assert_eq!(encode(Key::Shift_L, ModifierType::SHIFT_MASK, false), None);
}

#[test]
fn window_chords_accept_shifted_keyvals_and_ignore_lock() {
    let chord = ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK | ModifierType::LOCK_MASK;
    for (key, expected) in [
        (Key::D, Action::SplitVertical),
        (Key::E, Action::SplitHorizontal),
        (Key::W, Action::Close),
        (Key::Z, Action::Zoom),
        (Key::C, Action::CopyScreen),
        (Key::V, Action::Paste),
    ] {
        assert_eq!(action(key, chord), Some(expected));
    }
    assert_eq!(action(Key::v, ModifierType::CONTROL_MASK), None);
    assert_eq!(action(Key::V, chord | ModifierType::ALT_MASK), None);
}

#[test]
fn preedit_commit_routing_never_duplicates_text_and_key_events() {
    use crate::keyboard::{route_commit, CommitRoute};
    assert_eq!(route_commit(true, false, false), CommitRoute::Consumed);
    assert_eq!(route_commit(false, true, false), CommitRoute::Text);
    assert_eq!(route_commit(true, true, false), CommitRoute::Text);
    assert_eq!(route_commit(true, true, true), CommitRoute::Key);
    assert_eq!(route_commit(false, false, false), CommitRoute::Key);
}

#[test]
fn committed_text_is_sent_once_as_utf8_and_empty_commit_is_ignored() {
    assert_eq!(
        crate::keyboard::commit_text("你好e\u{301}", 0),
        Some("你好e\u{301}".as_bytes().to_vec())
    );
    assert_eq!(crate::keyboard::commit_text("", 0), None);
}

#[test]
fn clipboard_text_uses_paste_without_client_side_terminal_rewriting() {
    let text = "你好\r\n\x1b[201~e\u{301}";
    assert_eq!(
        paste(9, text),
        Some(Message::Paste {
            pane_id: 9,
            text: text.into()
        })
    );
    assert_eq!(paste(9, ""), None);
}

#[test]
fn ime_cursor_uses_the_focused_pane_origin_and_logical_pixels() {
    assert_eq!(
        cursor_rect([400.0, 200.0], [400.0, 200.0], [20.0, 40.0], [3, 2], 2.0),
        [230, 140, 10, 20]
    );
    assert_eq!(
        cursor_rect([0.0, 0.0], [100.0, 60.0], [10.0, 20.0], [200, 100], 1.0),
        [90, 40, 10, 20]
    );
}
