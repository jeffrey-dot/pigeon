//! Shape one core-owned cell without letting font advances change terminal geometry.

use std::borrow::Cow;
use std::collections::HashMap;

use swash::shape::ShapeContext;
use swash::text::{Codepoint, Script};
use swash::CacheKey;

use super::font::Font;

#[derive(Clone, Debug, PartialEq)]
pub struct ShapedGlyph {
    pub face: u8,
    pub glyph: u16,
    pub x: f32,
    pub y: f32,
}

const MAX_ENTRIES: usize = 1024;
const MAX_BYTES: usize = 1024 * 1024;

/// Font/size-scoped results; colours and synthetic bold do not change glyph positions.
#[derive(Default)]
pub struct ShapedCell {
    context: ShapeContext,
    font: Option<(CacheKey, u32)>,
    entries: HashMap<String, Box<[ShapedGlyph]>>,
    bytes: usize,
    #[cfg(test)]
    shapes: usize,
}

impl ShapedCell {
    pub fn get(&mut self, font: &Font, text: &str) -> Cow<'_, [ShapedGlyph]> {
        let identity = font.face(0).map(|face| (face.key, font.size.to_bits()));
        if self.font != identity {
            *self = Self::default();
            self.font = identity;
        }
        if !self.entries.contains_key(text) {
            #[cfg(test)]
            {
                self.shapes += 1;
            }
            let glyphs = shape(&mut self.context, font, text);
            // Include key/glyph allocations and a conservative hash-entry allowance.
            let bytes = text.len() + glyphs.len() * std::mem::size_of::<ShapedGlyph>() + 64;
            if bytes > MAX_BYTES {
                return Cow::Owned(glyphs);
            }
            if self.entries.len() >= MAX_ENTRIES || self.bytes + bytes > MAX_BYTES {
                self.entries.clear();
                self.bytes = 0;
            }
            self.bytes += bytes;
            self.entries
                .insert(text.to_owned(), glyphs.into_boxed_slice());
        }
        Cow::Borrowed(self.entries.get(text).map_or(&[], |glyphs| glyphs.as_ref()))
    }
}

fn shape(context: &mut ShapeContext, font: &Font, text: &str) -> Vec<ShapedGlyph> {
    // Prefer a face covering the whole cell, so mark positioning uses one font's anchors.
    let face = (0..font.face_count())
        .find(|index| {
            font.face(*index).is_some_and(|face| {
                text.chars().all(|ch| {
                    face.charmap().map(ch) != 0
                        || ch == '\u{200d}'
                        || ('\u{fe00}'..='\u{fe0f}').contains(&ch)
                })
            })
        })
        .or_else(|| {
            text.chars()
                .next()
                .and_then(|ch| font.glyph(ch).map(|(face, _)| face))
        });
    let Some((face, font_ref)) = face.and_then(|index| font.face(index).map(|face| (index, face)))
    else {
        return Vec::new();
    };
    let mut shaper = context
        .builder(font_ref)
        .size(font.size)
        .script(
            text.chars()
                .map(Codepoint::script)
                .find(|script| !matches!(script, Script::Common | Script::Inherited))
                .unwrap_or(Script::Latin),
        )
        .build();
    shaper.add_str(text);
    let mut pen = 0.0;
    let mut glyphs = Vec::new();
    shaper.shape_with(|cluster| {
        for glyph in cluster.glyphs {
            glyphs.push(ShapedGlyph {
                face,
                glyph: glyph.id,
                x: pen + glyph.x,
                y: glyph.y,
            });
            pen += glyph.advance;
        }
    });
    glyphs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_loaded_face_keeps_its_swash_cache_identity() {
        let font = Font::load(13.0).expect("installed monospace font");
        let first = font.face(0).expect("primary face");
        let second = font.face(0).expect("same face");
        assert_eq!(first.key, second.key);
    }

    #[test]
    #[ignore = "manual release-mode shaping benchmark"]
    fn repeated_cell_shaping_benchmark() {
        let font = Font::load(13.0).expect("installed monospace font");
        let mut cache = ShapedCell::default();
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            let mut context = ShapeContext::new();
            std::hint::black_box(shape(&mut context, &font, std::hint::black_box("e\u{301}")));
        }
        eprintln!("10,000 uncached cells: {:?}", started.elapsed());
        let started = std::time::Instant::now();
        for _ in 0..10_000 {
            std::hint::black_box(cache.get(&font, std::hint::black_box("e\u{301}")));
        }
        eprintln!(
            "10,000 cached cells: {:?}; shapes={}",
            started.elapsed(),
            cache.shapes
        );
    }

    #[test]
    fn repeated_cells_shape_once_and_font_size_changes_invalidate() {
        let mut font = Font::load(13.0).expect("installed monospace font");
        let mut cache = ShapedCell::default();
        let expected = shape(&mut ShapeContext::new(), &font, "e\u{301}");
        assert!(!expected.is_empty());
        for _ in 0..100 {
            assert_eq!(cache.get(&font, "e\u{301}").as_ref(), expected);
        }
        assert_eq!(cache.shapes, 1);
        font.size = 26.0;
        let resized = shape(&mut ShapeContext::new(), &font, "e\u{301}");
        assert_eq!(cache.get(&font, "e\u{301}").as_ref(), resized);
        let second = Font::load(26.0).expect("new font identity");
        cache.get(&second, "a\u{301}");
        assert_eq!(cache.entries.len(), 1);
        assert!(!cache.entries.contains_key("e\u{301}"));
    }

    #[test]
    fn changing_text_stays_within_entry_and_byte_budgets() {
        let font = Font::load(13.0).expect("installed monospace font");
        let mut cache = ShapedCell::default();
        for index in 0..MAX_ENTRIES + 20 {
            cache.get(&font, &format!("e\u{301}{index}"));
            assert!(cache.entries.len() <= MAX_ENTRIES);
            assert!(cache.bytes <= MAX_BYTES);
        }
        // Long distinct combining strings hit the byte budget before the entry count.
        for index in 0..100 {
            let text = format!("e{}{}", "\u{301}".repeat(1800), index);
            cache.get(&font, &text);
            assert!(cache.bytes <= MAX_BYTES);
        }
        assert!(cache.entries.len() < 100);
    }
}
