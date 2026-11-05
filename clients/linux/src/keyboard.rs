//! Kitty keyboard protocol and xterm legacy input mapping, using protocol.md modes.
//! Spec: https://sw.kovidgoyal.net/kitty/keyboard-protocol/ (progressive enhancement).

use gtk4::gdk::{Key, ModifierType as M};
use std::collections::{HashMap, HashSet};

pub mod gtk;
mod table;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Press,
    Repeat,
    Release,
}

#[derive(Clone)]
pub struct Stroke {
    pub key: Key,
    pub modifiers: M,
    pub unshifted: Option<char>,
    pub base: Option<char>,
    pub text: Option<String>,
}

#[derive(Default)]
pub struct Keyboard {
    held: HashSet<u32>,
    sent: HashMap<u32, (Stroke, u32)>,
}

impl Keyboard {
    pub fn press(&mut self, code: u32) -> Event {
        if self.held.insert(code) {
            Event::Press
        } else {
            Event::Repeat
        }
    }
    pub fn held(&self, code: u32) -> bool {
        self.held.contains(&code)
    }
    pub fn sent(&mut self, code: u32, stroke: Stroke, modes: u32) {
        self.sent.insert(code, (stroke, modes));
    }
    pub fn release(&mut self, code: u32, modifiers: M, modes: u32) -> Option<Vec<u8>> {
        self.held.remove(&code);
        let (mut stroke, original) = self.sent.remove(&code)?;
        // A protocol reset between press/release must not inject stale escape sequences.
        if (original ^ modes) & (31 << 10) != 0 {
            return None;
        }
        encode(&stroke, Event::Release, original)?;
        stroke.modifiers = modifiers;
        encode_event(&stroke, Event::Release, modes, true)
    }
    pub fn cancel(&mut self, modes: u32) -> Vec<u8> {
        self.held.clear();
        let mut bytes = Vec::new();
        for (_, (mut stroke, original)) in self.sent.drain() {
            if (original ^ modes) & (31 << 10) == 0
                && encode(&stroke, Event::Release, original).is_some()
            {
                stroke.modifiers = M::empty();
                bytes
                    .extend(encode_event(&stroke, Event::Release, modes, true).unwrap_or_default());
            }
        }
        bytes
    }
    pub fn other_modifier(&self, key: Key) -> bool {
        let bit = table::modifier(key).map(|(_, bit)| bit);
        self.sent
            .values()
            .any(|(s, _)| s.key != key && table::modifier(s.key).map(|(_, b)| b) == bit)
    }
}

pub fn modifier_state(key: Key, state: M, pressed: bool, other_held: bool) -> M {
    let Some((_, bit)) = table::modifier(key) else {
        return state;
    };
    if key == Key::Caps_Lock {
        return if pressed { state ^ M::LOCK_MASK } else { state };
    }
    if pressed || other_held {
        state | bit
    } else {
        state - bit
    }
}

pub fn encode(stroke: &Stroke, event: Event, modes: u32) -> Option<Vec<u8>> {
    encode_event(stroke, event, modes, false)
}
fn encode_event(stroke: &Stroke, event: Event, modes: u32, paired: bool) -> Option<Vec<u8>> {
    let flags = (modes >> 10) & 31;
    if event == Event::Release && flags & 2 == 0 {
        return None;
    }
    if flags == 0 {
        return legacy(stroke, modes);
    }
    let all = flags & 8 != 0;
    let disambiguate = all || flags & 1 != 0;
    let mut modifiers = stroke.modifiers;
    let key = stroke.key;
    let code = if let Some((code, _)) = table::modifier(key) {
        if !all {
            return None;
        }
        (code.to_string(), 'u')
    } else if matches!(
        key,
        Key::Return | Key::KP_Enter | Key::Tab | Key::ISO_Left_Tab | Key::BackSpace
    ) {
        if event == Event::Release && !all {
            return None;
        }
        if !all && (!disambiguate || (modifiers - M::LOCK_MASK).is_empty()) {
            return legacy(stroke, modes);
        }
        (
            match key {
                Key::KP_Enter => 57414,
                Key::Return => 13,
                Key::BackSpace => 127,
                _ => 9,
            }
            .to_string(),
            'u',
        )
    } else if key == Key::Escape {
        if !disambiguate {
            return if event == Event::Release {
                None
            } else {
                legacy(stroke, modes)
            };
        }
        ("27".into(), 'u')
    } else if let Some(code) = table::functional(key) {
        if !all && code.1 != 'u' && event == Event::Press && (modifiers - M::LOCK_MASK).is_empty() {
            return legacy(stroke, modes);
        }
        (code.0.to_string(), code.1)
    } else {
        let typed = key.to_unicode().filter(|c| !c.is_control())?;
        if !all {
            modifiers -= M::LOCK_MASK;
        }
        let chord =
            modifiers.intersects(M::CONTROL_MASK | M::ALT_MASK | M::SUPER_MASK | M::META_MASK);
        if !paired && !all && !(disambiguate && chord) {
            return if event == Event::Release {
                None
            } else {
                legacy(stroke, modes)
            };
        }
        let plain = table::keypad(key).map(|(code, _)| code).unwrap_or_else(|| {
            stroke
                .unshifted
                .or_else(|| key.to_lower().to_unicode())
                .unwrap_or(typed) as u32
        });
        let mut code = plain.to_string();
        if flags & 4 != 0 {
            let shifted = if modifiers.contains(M::SHIFT_MASK) && typed as u32 != plain {
                (typed as u32).to_string()
            } else {
                String::new()
            };
            let base = stroke.base.filter(|c| *c as u32 != plain);
            if let Some(base) = base {
                code.push_str(&format!(":{shifted}:{}", base as u32));
            } else if !shifted.is_empty() {
                code.push_str(&format!(":{shifted}"));
            }
        }
        (code, 'u')
    };
    let text = if all && flags & 16 != 0 && event != Event::Release {
        stroke
            .text
            .as_deref()
            .map(text_points)
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    Some(sequence(
        &code.0,
        code.1,
        modifiers,
        event,
        flags,
        text.as_deref(),
    ))
}

pub fn commit_text(text: &str, modes: u32) -> Option<Vec<u8>> {
    if text.is_empty() {
        return None;
    }
    let flags = (modes >> 10) & 31;
    if flags & 8 == 0 {
        return Some(text.as_bytes().to_vec());
    }
    let points = text_points(text);
    (flags & 16 != 0 && !points.is_empty()).then(|| format!("\x1b[0;;{points}u").into_bytes())
}
fn text_points(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_control())
        .map(|c| (c as u32).to_string())
        .collect::<Vec<_>>()
        .join(":")
}
fn sequence(
    code: &str,
    final_char: char,
    modifiers: M,
    event: Event,
    flags: u32,
    text: Option<&str>,
) -> Vec<u8> {
    let mut bits = 0;
    for (mask, bit) in [
        (M::SHIFT_MASK, 1),
        (M::ALT_MASK, 2),
        (M::CONTROL_MASK, 4),
        (M::SUPER_MASK, 8),
        (M::HYPER_MASK, 16),
        (M::META_MASK, 32),
        (M::LOCK_MASK, 64),
    ] {
        if modifiers.contains(mask) {
            bits |= bit;
        }
    }
    let event_type = if flags & 2 != 0 {
        match event {
            Event::Press => None,
            Event::Repeat => Some(2),
            Event::Release => Some(3),
        }
    } else {
        None
    };
    let mut field = if bits != 0 || event_type.is_some() {
        (bits + 1).to_string()
    } else {
        String::new()
    };
    if let Some(kind) = event_type {
        field.push_str(&format!(":{kind}"));
    }
    if code == "1" && final_char != 'u' && field.is_empty() && text.is_none() {
        return format!("\x1b[{final_char}").into_bytes();
    }
    let mut out = format!("\x1b[{code}");
    if !field.is_empty() || text.is_some() {
        out.push(';');
        out.push_str(&field);
    }
    if let Some(text) = text {
        out.push(';');
        out.push_str(text);
    }
    out.push(final_char);
    out.into_bytes()
}
fn legacy(stroke: &Stroke, modes: u32) -> Option<Vec<u8>> {
    if modes & 128 != 0 {
        if let Some((_, suffix)) = table::keypad(stroke.key) {
            return Some(vec![27, b'O', suffix]);
        }
    }
    if stroke.key == Key::ISO_Left_Tab
        || (stroke.key == Key::Tab && stroke.modifiers.contains(M::SHIFT_MASK))
    {
        return Some(b"\x1b[Z".to_vec());
    }
    crate::input::encode(
        table::legacy_key(stroke.key),
        stroke.modifiers,
        modes & 1 != 0,
    )
}
