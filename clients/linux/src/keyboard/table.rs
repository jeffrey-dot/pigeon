//! Key assignments from the Kitty functional key table; GDK supplies key identity.
use gtk4::gdk::{Key, ModifierType as M};
use gtk4::glib::translate::IntoGlib;
pub fn modifier(key: Key) -> Option<(u32, M)> {
    Some(match key {
        Key::Shift_L => (57441, M::SHIFT_MASK),
        Key::Shift_R => (57447, M::SHIFT_MASK),
        Key::Control_L => (57442, M::CONTROL_MASK),
        Key::Control_R => (57448, M::CONTROL_MASK),
        Key::Alt_L => (57443, M::ALT_MASK),
        Key::Alt_R => (57449, M::ALT_MASK),
        Key::Super_L => (57444, M::SUPER_MASK),
        Key::Super_R => (57450, M::SUPER_MASK),
        Key::Hyper_L => (57445, M::HYPER_MASK),
        Key::Hyper_R => (57451, M::HYPER_MASK),
        Key::Meta_L => (57446, M::META_MASK),
        Key::Meta_R => (57452, M::META_MASK),
        Key::Caps_Lock => (57358, M::LOCK_MASK),
        _ => return None,
    })
}
pub fn functional(key: Key) -> Option<(u32, char)> {
    Some(match key {
        Key::Up => (1, 'A'),
        Key::Down => (1, 'B'),
        Key::Right => (1, 'C'),
        Key::Left => (1, 'D'),
        Key::Home => (1, 'H'),
        Key::End => (1, 'F'),
        Key::Insert => (2, '~'),
        Key::Delete => (3, '~'),
        Key::Page_Up => (5, '~'),
        Key::Page_Down => (6, '~'),
        Key::F1 => (1, 'P'),
        Key::F2 => (1, 'Q'),
        Key::F3 => (13, '~'),
        Key::F4 => (1, 'S'),
        Key::F5 => (15, '~'),
        Key::F6 => (17, '~'),
        Key::F7 => (18, '~'),
        Key::F8 => (19, '~'),
        Key::F9 => (20, '~'),
        Key::F10 => (21, '~'),
        Key::F11 => (23, '~'),
        Key::F12 => (24, '~'),
        Key::KP_Left => (57417, 'u'),
        Key::KP_Right => (57418, 'u'),
        Key::KP_Up => (57419, 'u'),
        Key::KP_Down => (57420, 'u'),
        Key::KP_Page_Up => (57421, 'u'),
        Key::KP_Page_Down => (57422, 'u'),
        Key::KP_Home => (57423, 'u'),
        Key::KP_End => (57424, 'u'),
        Key::KP_Insert => (57425, 'u'),
        Key::KP_Delete => (57426, 'u'),
        Key::KP_Begin => (57427, 'u'),
        Key::Caps_Lock => (57358, 'u'),
        Key::Scroll_Lock => (57359, 'u'),
        Key::Num_Lock => (57360, 'u'),
        Key::Print => (57361, 'u'),
        Key::Pause => (57362, 'u'),
        Key::Menu => (57363, 'u'),
        _ if key >= Key::F13 && key <= Key::F35 => {
            (57376 + key.into_glib() - Key::F13.into_glib(), 'u')
        }
        _ => return None,
    })
}
pub fn keypad(key: Key) -> Option<(u32, u8)> {
    Some(match key {
        Key::KP_0 | Key::KP_Insert => (57399, b'p'),
        Key::KP_1 | Key::KP_End => (57400, b'q'),
        Key::KP_2 | Key::KP_Down => (57401, b'r'),
        Key::KP_3 | Key::KP_Page_Down => (57402, b's'),
        Key::KP_4 | Key::KP_Left => (57403, b't'),
        Key::KP_5 | Key::KP_Begin => (57404, b'u'),
        Key::KP_6 | Key::KP_Right => (57405, b'v'),
        Key::KP_7 | Key::KP_Home => (57406, b'w'),
        Key::KP_8 | Key::KP_Up => (57407, b'x'),
        Key::KP_9 | Key::KP_Page_Up => (57408, b'y'),
        Key::KP_Decimal | Key::KP_Delete => (57409, b'n'),
        Key::KP_Divide => (57410, b'o'),
        Key::KP_Multiply => (57411, b'j'),
        Key::KP_Subtract => (57412, b'm'),
        Key::KP_Add => (57413, b'k'),
        Key::KP_Enter => (57414, b'M'),
        Key::KP_Equal => (57415, b'X'),
        Key::KP_Separator => (57416, b'l'),
        _ => return None,
    })
}
pub fn legacy_key(key: Key) -> Key {
    match key {
        Key::KP_Left => Key::Left,
        Key::KP_Right => Key::Right,
        Key::KP_Up => Key::Up,
        Key::KP_Down => Key::Down,
        Key::KP_Page_Up => Key::Page_Up,
        Key::KP_Page_Down => Key::Page_Down,
        Key::KP_Home => Key::Home,
        Key::KP_End => Key::End,
        Key::KP_Insert => Key::Insert,
        Key::KP_Delete => Key::Delete,
        _ => key,
    }
}
