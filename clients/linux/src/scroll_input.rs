//! Pure navigation mapping; modified shell keys remain terminal input.

use gtk4::gdk::{Key, ModifierType};

pub fn page(
    key: Key,
    modifiers: ModifierType,
    active: bool,
    alt: bool,
    rows: usize,
) -> Option<i32> {
    let modifiers = modifiers - ModifierType::LOCK_MASK;
    if alt || !(modifiers == ModifierType::SHIFT_MASK || (active && modifiers.is_empty())) {
        return None;
    }
    let lines = rows.saturating_sub(1).max(1).min(i32::MAX as usize) as i32;
    match key {
        Key::Page_Up => Some(-lines),
        Key::Page_Down => Some(lines),
        Key::End if modifiers == ModifierType::SHIFT_MASK => Some(0),
        Key::Home if modifiers == ModifierType::SHIFT_MASK => Some(-i32::MAX),
        _ => None,
    }
}

#[derive(Default)]
pub struct Wheel {
    remainder: f64,
}

impl Wheel {
    pub fn reset(&mut self) {
        self.remainder = 0.0;
    }

    /// Surface deltas are logical pixels; wheel detents move three rows.
    pub fn lines(&mut self, delta: f64, surface: bool, cell_height: f64) -> i32 {
        if !delta.is_finite() || !cell_height.is_finite() || cell_height <= 0.0 {
            self.reset();
            return 0;
        }
        let delta = if surface {
            delta / cell_height
        } else {
            delta * 3.0
        };
        self.remainder = (self.remainder + delta).clamp(-10000.0, 10000.0);
        let lines = self.remainder.trunc() as i32;
        self.remainder -= f64::from(lines);
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_keys_do_not_steal_shell_or_alternate_screen_bindings() {
        let none = ModifierType::empty();
        let shift = ModifierType::SHIFT_MASK;
        assert_eq!(page(Key::Page_Up, none, false, false, 24), None);
        assert_eq!(page(Key::Page_Up, shift, false, false, 24), Some(-23));
        assert_eq!(page(Key::Page_Down, none, true, false, 24), Some(23));
        assert_eq!(page(Key::End, shift, true, false, 24), Some(0));
        assert_eq!(page(Key::Home, shift, false, false, 24), Some(-i32::MAX));
        for modifiers in [
            none,
            shift,
            ModifierType::CONTROL_MASK,
            ModifierType::ALT_MASK,
        ] {
            assert_eq!(page(Key::Page_Up, modifiers, true, true, 24), None);
        }
        assert_eq!(
            page(Key::Page_Up, ModifierType::CONTROL_MASK, true, false, 24),
            None
        );
        assert_eq!(page(Key::Home, none, true, false, 24), None);
    }

    #[test]
    fn wheel_accumulates_fractional_rows_and_resets_between_targets() {
        let mut wheel = Wheel::default();
        assert_eq!(wheel.lines(-1.0, false, 16.0), -3);
        assert_eq!(wheel.lines(-6.0, true, 16.0), 0);
        assert_eq!(wheel.lines(-6.0, true, 16.0), 0);
        assert_eq!(wheel.lines(-6.0, true, 16.0), -1);
        assert_eq!(wheel.lines(18.0, true, 16.0), 1);
        wheel.lines(10.0, true, 16.0);
        wheel.reset();
        assert_eq!(wheel.lines(6.0, true, 16.0), 0);
        assert_eq!(wheel.lines(f64::NAN, true, 16.0), 0);
        assert_eq!(wheel.lines(1.0, true, 0.0), 0);
    }
}
