use super::*;
use gtk4::gdk::{Key, ModifierType as M};

fn stroke(key: Key, modifiers: M) -> Stroke {
    Stroke {
        key,
        modifiers,
        unshifted: None,
        base: None,
        text: None,
    }
}
fn bytes(key: Key, modifiers: M, flags: u32, event: Event) -> Option<Vec<u8>> {
    encode(&stroke(key, modifiers), event, flags << 10)
}
#[test]
fn kitty_negotiated_events_and_legacy_rescue_keys() {
    assert_eq!(
        bytes(Key::Return, M::SHIFT_MASK, 1, Event::Press),
        Some(b"\x1b[13;2u".to_vec())
    );
    assert_eq!(bytes(Key::Return, M::empty(), 3, Event::Release), None);
    assert_eq!(bytes(Key::a, M::empty(), 3, Event::Release), None);
    assert_eq!(
        bytes(Key::a, M::empty(), 11, Event::Repeat),
        Some(b"\x1b[97;1:2u".to_vec())
    );
    assert_eq!(
        bytes(Key::a, M::empty(), 11, Event::Release),
        Some(b"\x1b[97;1:3u".to_vec())
    );
    assert_eq!(
        bytes(Key::F3, M::CONTROL_MASK, 1, Event::Press),
        Some(b"\x1b[13;5~".to_vec())
    );
    assert_eq!(
        bytes(Key::Escape, M::empty(), 1, Event::Press),
        Some(b"\x1b[27u".to_vec())
    );
    assert_eq!(bytes(Key::a, M::empty(), 0, Event::Release), None);
    assert_eq!(
        encode(&stroke(Key::Up, M::empty()), Event::Press, 1),
        Some(b"\x1bOA".to_vec())
    );
}
#[test]
fn layout_alternates_and_associated_text_do_not_guess_us_punctuation() {
    let mut key = stroke(Key::exclam, M::SHIFT_MASK);
    key.unshifted = Some('1');
    key.base = Some('2');
    key.text = Some("!".into());
    assert_eq!(
        encode(&key, Event::Press, 31 << 10),
        Some(b"\x1b[49:33:50;2;33u".to_vec())
    );
    key.text = Some("\0\n\u{7f}\u{85}".into());
    assert_eq!(
        encode(&key, Event::Press, 31 << 10),
        Some(b"\x1b[49:33:50;2u".to_vec())
    );
    assert_eq!(
        commit_text("你好", 24 << 10),
        Some(b"\x1b[0;;20320:22909u".to_vec())
    );
    assert_eq!(commit_text("你好", 8 << 10), None);
    assert_eq!(commit_text("", 0), None);
}
#[test]
fn application_keypad_and_shift_tab_are_preserved() {
    assert_eq!(
        encode(&stroke(Key::KP_1, M::empty()), Event::Press, 128),
        Some(b"\x1bOq".to_vec())
    );
    assert_eq!(
        encode(&stroke(Key::KP_Enter, M::empty()), Event::Press, 128),
        Some(b"\x1bOM".to_vec())
    );
    assert_eq!(
        bytes(Key::KP_1, M::empty(), 8, Event::Press),
        Some(b"\x1b[57400u".to_vec())
    );
    assert_eq!(
        bytes(Key::ISO_Left_Tab, M::SHIFT_MASK, 0, Event::Press),
        Some(b"\x1b[Z".to_vec())
    );
}
#[test]
fn releases_require_an_emitted_press_and_keep_its_key_identity() {
    let mut keys = Keyboard::default();
    let mode = 11 << 10;
    assert_eq!(keys.release(38, M::empty(), mode), None);
    assert_eq!(keys.press(38), Event::Press);
    assert_eq!(keys.press(38), Event::Repeat);
    keys.sent(38, stroke(Key::a, M::empty()), mode);
    assert_eq!(
        keys.release(38, M::empty(), mode),
        Some(b"\x1b[97;1:3u".to_vec())
    );
    keys.press(40);
    assert_eq!(keys.release(40, M::empty(), mode), None);
    keys.sent(38, stroke(Key::a, M::empty()), mode);
    assert_eq!(keys.release(38, M::empty(), 0), None);
}

#[test]
fn chord_release_is_reported_even_when_modifier_was_released_first() {
    let mut keys = Keyboard::default();
    keys.sent(38, stroke(Key::a, M::CONTROL_MASK), 3 << 10);
    assert_eq!(
        keys.release(38, M::empty(), 3 << 10),
        Some(b"\x1b[97;1:3u".to_vec())
    );
    keys.sent(38, stroke(Key::a, M::empty()), 3 << 10);
    assert_eq!(keys.release(38, M::CONTROL_MASK, 3 << 10), None);
}

#[test]
fn paired_releases_survive_local_cancellation_and_never_add_associated_text() {
    let mut keys = Keyboard::default();
    let mut key = stroke(Key::a, M::CONTROL_MASK);
    key.text = Some("a".into());
    keys.sent(38, key, 31 << 10);
    assert_eq!(keys.cancel(31 << 10), b"\x1b[97;1:3u");
    assert_eq!(keys.release(38, M::empty(), 31 << 10), None);
}

#[test]
fn opposite_modifier_release_keeps_the_remaining_modifier_bit() {
    let mut keys = Keyboard::default();
    keys.sent(50, stroke(Key::Shift_L, M::SHIFT_MASK), 11 << 10);
    keys.sent(62, stroke(Key::Shift_R, M::SHIFT_MASK), 11 << 10);
    assert_eq!(
        modifier_state(
            Key::Shift_L,
            M::SHIFT_MASK,
            false,
            keys.other_modifier(Key::Shift_L)
        ),
        M::SHIFT_MASK
    );
    assert_eq!(
        keys.release(50, M::SHIFT_MASK, 11 << 10),
        Some(b"\x1b[57441;2:3u".to_vec())
    );
    assert_eq!(
        modifier_state(
            Key::Shift_R,
            M::SHIFT_MASK,
            false,
            keys.other_modifier(Key::Shift_R)
        ),
        M::empty()
    );
}
