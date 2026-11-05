use super::*;
use gtk4::gdk::ModifierType as M;
#[test]
fn mouse_modes_coordinates_modifiers_and_invalid_reports() {
    let point = [2, 4];
    assert_eq!(
        encode(0, Event::Press, point, M::CONTROL_MASK, 4 | 32),
        Some(b"\x1b[<16;3;5M".to_vec())
    );
    assert_eq!(
        encode(0, Event::Release, point, M::empty(), 4 | 32),
        Some(b"\x1b[<0;3;5m".to_vec())
    );
    assert_eq!(encode(0, Event::Drag, point, M::empty(), 4), None);
    assert_eq!(encode(3, Event::Move, point, M::empty(), 8), None);
    assert_eq!(
        encode(3, Event::Move, point, M::empty(), 16 | 32),
        Some(b"\x1b[<35;3;5M".to_vec())
    );
    assert_eq!(
        encode(64, Event::Press, point, M::empty(), 4),
        Some(vec![27, 91, 77, 96, 35, 37])
    );
    assert_eq!(
        encode(0, Event::Release, point, M::empty(), 4),
        Some(vec![27, 91, 77, 35, 35, 37])
    );
    assert_eq!(encode(0, Event::Press, [223, 0], M::empty(), 4), None);
    assert_eq!(encode(99, Event::Press, point, M::empty(), 4 | 32), None);
    assert_eq!(encode(0, Event::Press, point, M::empty(), 0), None);
}
