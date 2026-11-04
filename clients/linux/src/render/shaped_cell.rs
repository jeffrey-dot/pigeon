//! Shape one core-owned cell without letting font advances change terminal geometry.

use swash::shape::ShapeContext;
use swash::text::{Codepoint, Script};

use super::font::Font;

pub struct ShapedGlyph {
    pub face: u8,
    pub glyph: u16,
    pub x: f32,
    pub y: f32,
}

pub fn shape(font: &Font, text: &str) -> Vec<ShapedGlyph> {
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
    let mut context = ShapeContext::new();
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
