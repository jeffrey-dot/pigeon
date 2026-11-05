//! GDK keymap data, without assuming US punctuation or a fixed hardware keycode table.
use super::Stroke;
use gtk4::{gdk, prelude::*};

pub fn stroke(
    controller: &gtk4::EventControllerKey,
    key: gdk::Key,
    code: u32,
    modifiers: gdk::ModifierType,
) -> Stroke {
    let event = controller
        .current_event()
        .and_then(|e| e.downcast::<gdk::KeyEvent>().ok());
    let display = controller.widget().map(|w| w.display());
    let unshifted = display
        .as_ref()
        .and_then(|d| {
            d.translate_key(
                code,
                gdk::ModifierType::empty(),
                event.as_ref().map_or(0, |e| e.layout() as i32),
            )
        })
        .and_then(|(k, _, _, _)| k.to_unicode())
        .filter(|c| !c.is_control());
    // Only expose a base key when GDK actually supplies an ASCII layout for this key.
    let base = display.and_then(|d| d.map_keycode(code)).and_then(|keys| {
        keys.into_iter().find_map(|(map, key)| {
            key.to_unicode()
                .filter(|c| map.level() == 0 && c.is_ascii() && !c.is_control())
        })
    });
    Stroke {
        key,
        modifiers,
        unshifted,
        base,
        text: None,
    }
}
