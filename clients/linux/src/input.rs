//! Key events → terminal bytes, xterm's legacy encoding.
//!
//! This is the counterpart of the macOS client's `KeyEncoder` (which is unit-tested on that
//! side); the kitty keyboard protocol is a later step (`docs/renderer.md` §5.4).
//! Source of the sequences: xterm's ctlseqs, as every readline/vim binding expects them.

use gtk4::gdk::{Key, ModifierType};
use pigeon_core::api::{Message, PaneId};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    SplitVertical,
    SplitHorizontal,
    Close,
    Zoom,
    CopyScreen,
    Paste,
}

pub fn action(key: Key, modifiers: ModifierType) -> Option<Action> {
    if !modifiers.contains(ModifierType::CONTROL_MASK | ModifierType::SHIFT_MASK)
        || modifiers
            .intersects(ModifierType::ALT_MASK | ModifierType::SUPER_MASK | ModifierType::META_MASK)
    {
        return None;
    }
    match key.to_lower() {
        Key::d => Some(Action::SplitVertical),
        Key::e => Some(Action::SplitHorizontal),
        Key::w => Some(Action::Close),
        Key::z => Some(Action::Zoom),
        Key::c => Some(Action::CopyScreen),
        Key::v => Some(Action::Paste),
        _ => None,
    }
}

/// Only the unconsumed event may fall back to legacy encoding.
pub fn fallback(
    key: Key,
    modifiers: ModifierType,
    app_cursor: bool,
    im_handled: bool,
    im_committed: bool,
) -> Option<Vec<u8>> {
    if im_handled || im_committed {
        return None;
    }
    encode(key, modifiers, app_cursor)
}

pub fn commit(pane_id: PaneId, text: &str) -> Option<Message> {
    if text.is_empty() {
        return None;
    }
    Some(Message::Keys {
        pane_id,
        bytes: text.as_bytes().to_vec(),
    })
}

pub fn paste(pane_id: PaneId, text: &str) -> Option<Message> {
    if text.is_empty() {
        return None;
    }
    Some(Message::Paste {
        pane_id,
        text: text.to_owned(),
    })
}

/// GTK IM coordinates are widget-local logical pixels; grid geometry uses device pixels.
pub fn cursor_rect(
    origin: [f32; 2],
    size: [f32; 2],
    cell: [f32; 2],
    cursor: [u16; 2],
    scale: f32,
) -> [i32; 4] {
    let scale = scale.max(1.0);
    let x = (f32::from(cursor[0]) * cell[0]).min((size[0] - cell[0]).max(0.0));
    let y = (f32::from(cursor[1]) * cell[1]).min((size[1] - cell[1]).max(0.0));
    [
        ((origin[0] + x) / scale).floor() as i32,
        ((origin[1] + y) / scale).floor() as i32,
        (cell[0] / scale).ceil().max(1.0) as i32,
        (cell[1] / scale).ceil().max(1.0) as i32,
    ]
}

/// The `CSI 1;<modifier>` parameter: 1 + shift + 2·alt + 4·ctrl, and 1 means "no modifier".
fn modifier_parameter(modifiers: ModifierType) -> u8 {
    let mut value = 1u8;
    if modifiers.contains(ModifierType::SHIFT_MASK) {
        value += 1;
    }
    if modifiers.contains(ModifierType::ALT_MASK) {
        value += 2;
    }
    if modifiers.contains(ModifierType::CONTROL_MASK) {
        value += 4;
    }
    value
}

/// `None` means "this key produces nothing" (a modifier alone, or a chord the app owns).
pub fn encode(key: Key, modifiers: ModifierType, app_cursor: bool) -> Option<Vec<u8>> {
    let control = modifiers.contains(ModifierType::CONTROL_MASK);
    let alt = modifiers.contains(ModifierType::ALT_MASK);
    let modifier = modifier_parameter(modifiers);

    if let Some(sequence) = special(key, modifier, app_cursor) {
        return Some(sequence);
    }

    let character = key.to_unicode()?;
    // Control characters come from the letter, whichever case the keyboard reported.
    if control {
        if let Some(byte) = control_byte(character) {
            return Some(if alt { vec![0x1b, byte] } else { vec![byte] });
        }
    }
    let mut bytes = Vec::with_capacity(4);
    if alt {
        bytes.push(0x1b);
    }
    let mut buffer = [0u8; 4];
    bytes.extend_from_slice(character.encode_utf8(&mut buffer).as_bytes());
    Some(bytes)
}

fn special(key: Key, modifier: u8, app_cursor: bool) -> Option<Vec<u8>> {
    let plain = modifier == 1;
    let cursor = |final_byte: char| -> Vec<u8> {
        if plain && app_cursor {
            vec![0x1b, b'O', final_byte as u8]
        } else if plain {
            vec![0x1b, b'[', final_byte as u8]
        } else {
            format!("\x1b[1;{modifier}{final_byte}").into_bytes()
        }
    };
    let tilde = |number: &str| -> Vec<u8> {
        if plain {
            format!("\x1b[{number}~").into_bytes()
        } else {
            format!("\x1b[{number};{modifier}~").into_bytes()
        }
    };

    Some(match key {
        Key::Return | Key::KP_Enter => vec![0x0d],
        Key::Tab => vec![0x09],
        Key::BackSpace => vec![0x7f],
        Key::Escape => vec![0x1b],
        Key::Up => cursor('A'),
        Key::Down => cursor('B'),
        Key::Right => cursor('C'),
        Key::Left => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde("2"),
        Key::Delete => tilde("3"),
        Key::Page_Up => tilde("5"),
        Key::Page_Down => tilde("6"),
        Key::F1 => ss3_or_tilde('P', modifier),
        Key::F2 => ss3_or_tilde('Q', modifier),
        Key::F3 => ss3_or_tilde('R', modifier),
        Key::F4 => ss3_or_tilde('S', modifier),
        Key::F5 => tilde("15"),
        Key::F6 => tilde("17"),
        Key::F7 => tilde("18"),
        Key::F8 => tilde("19"),
        Key::F9 => tilde("20"),
        Key::F10 => tilde("21"),
        Key::F11 => tilde("23"),
        Key::F12 => tilde("24"),
        _ => return None,
    })
}

fn ss3_or_tilde(final_byte: char, modifier: u8) -> Vec<u8> {
    if modifier == 1 {
        vec![0x1b, b'O', final_byte as u8]
    } else {
        format!("\x1b[1;{modifier}{final_byte}").into_bytes()
    }
}

fn control_byte(character: char) -> Option<u8> {
    let byte = u8::try_from(character).ok()?;
    match byte {
        b'a'..=b'z' => Some(byte - b'a' + 1),
        b'A'..=b'Z' => Some(byte - b'A' + 1),
        b'[' | b'\\' | b']' | b'^' | b'_' | b'@' => Some(byte - 0x40),
        b'?' => Some(0x7f),
        b' ' => Some(0x00),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
