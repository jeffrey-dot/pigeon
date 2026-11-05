//! xterm mouse and focus reports (ctlseqs: 1000/1002/1003/1004/1006).
use gtk4::gdk::ModifierType as M;
#[cfg(test)]
mod tests;
pub const ANY_MOUSE: u32 = 4 | 8 | 16;
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Event {
    Press,
    Release,
    Drag,
    Move,
}
pub fn encode(
    button: u8,
    event: Event,
    point: [u16; 2],
    modifiers: M,
    modes: u32,
) -> Option<Vec<u8>> {
    if modes & ANY_MOUSE == 0 || !matches!(button, 0..=3 | 64..=65) {
        return None;
    }
    if event == Event::Drag && modes & (8 | 16) == 0 || event == Event::Move && modes & 16 == 0 {
        return None;
    }
    let mut code = button;
    if matches!(event, Event::Drag | Event::Move) {
        code += 32;
    }
    for (mask, bit) in [(M::SHIFT_MASK, 4), (M::ALT_MASK, 8), (M::CONTROL_MASK, 16)] {
        if modifiers.contains(mask) {
            code += bit;
        }
    }
    let x = u32::from(point[0]) + 1;
    let y = u32::from(point[1]) + 1;
    if modes & 32 != 0 {
        return Some(
            format!(
                "\x1b[<{code};{x};{y}{}",
                if event == Event::Release { 'm' } else { 'M' }
            )
            .into_bytes(),
        );
    }
    if x > 223 || y > 223 {
        return None;
    }
    if event == Event::Release {
        code = 3 + (code & !3 & !64);
    }
    Some(vec![27, b'[', b'M', 32 + code, 32 + x as u8, 32 + y as u8])
}
