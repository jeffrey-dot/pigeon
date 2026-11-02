//! Turning one screen into the two instance batches the shaders eat.
//!
//! Field order and sizes must match `MetalShaders`-style GLSL in `gl.rs`
//! (`docs/renderer.md` §5.3).

use super::atlas::{Atlas, Key};
use super::font::{Font, GridMetrics};
use super::palette::{decoration, Palette};
use crate::screen::Screen;

/// A solid rectangle in device pixels, top-left origin.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RectInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub color: [f32; 4],
}

impl RectInstance {
    pub const STRIDE: i32 = 32;
}

/// One glyph quad: where, how big, which atlas rectangle, in what colour.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct GlyphInstance {
    pub pos: [f32; 2],
    pub size: [f32; 2],
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

impl GlyphInstance {
    pub const STRIDE: i32 = 48;
}

#[derive(Default)]
pub struct Batch {
    pub rects: Vec<RectInstance>,
    pub glyphs: Vec<GlyphInstance>,
}

/// Draws every cell of `screen` plus the cursor.
///
/// The whole grid is rebuilt each frame; dirty-row tracking is the next optimisation, and
/// is worth doing only once `PIGEON_FRAME_STATS=1` says it matters (`docs/renderer.md` §5.4).
pub fn build(
    gl: &glow::Context,
    screen: &Screen,
    font: &Font,
    metrics: GridMetrics,
    palette: &Palette,
    atlas: &mut Atlas,
    cursor_on: bool,
) -> Batch {
    let cols = screen.cols();
    let rows = screen.rows();
    let mut batch = Batch::default();
    let cursor = &screen.snapshot.cursor;
    let cursor_cell = (usize::from(cursor.x), usize::from(cursor.y));

    for y in 0..rows {
        let top = y as f32 * metrics.cell_h;
        for x in 0..cols {
            let Some(cell) = screen.cell(x, y) else {
                break;
            };
            if cell.width == 0 {
                // Continuation of the wide char to the left; it owns the two cells.
                continue;
            }
            let attrs = cell.attrs;
            let (fg, bg) = palette.colors(cell.fg, cell.bg, attrs);
            let left = x as f32 * metrics.cell_w;
            let width = metrics.cell_w * f32::from(cell.width.max(1));

            if bg != palette.background {
                batch.rects.push(RectInstance {
                    pos: [left, top],
                    size: [width, metrics.cell_h],
                    color: Palette::components(bg),
                });
            }

            // A block cursor paints the cell in the foreground colour, so the glyph on top
            // of it has to switch to the background colour.
            let under_cursor =
                cursor_on && cursor.visible && cursor.shape == 0 && (x, y) == cursor_cell;
            let glyph_color = if under_cursor { bg } else { fg };

            if cell.codepoint > 0x20 && cell.codepoint != 0x7f {
                if let Some(ch) = char::from_u32(cell.codepoint) {
                    if let Some((face, glyph)) = font.glyph(ch) {
                        let key = Key {
                            face,
                            glyph,
                            bold: attrs & pigeon_core::api::ATTR_BOLD != 0,
                        };
                        if let Some(slot) = atlas.slot(gl, font, key) {
                            let baseline = top + metrics.baseline;
                            batch.glyphs.push(GlyphInstance {
                                pos: [left + slot.left as f32, baseline - slot.top as f32],
                                size: [slot.w as f32, slot.h as f32],
                                uv: slot.uv(),
                                color: Palette::components(glyph_color),
                            });
                        }
                    }
                }
            }

            if let Some(attr) = decoration(attrs) {
                let thickness = (metrics.cell_h * 0.06).max(1.0);
                let y_offset = if attr == pigeon_core::api::ATTR_UNDERLINE {
                    metrics.baseline + (metrics.cell_h * 0.08).max(1.0)
                } else {
                    metrics.baseline - metrics.cell_h * 0.25
                };
                batch.rects.push(RectInstance {
                    pos: [left, top + y_offset],
                    size: [width, thickness],
                    color: Palette::components(fg),
                });
            }
        }
    }

    if cursor_on && cursor.visible {
        let (cx, cy) = cursor_cell;
        let left = cx as f32 * metrics.cell_w;
        let top = cy as f32 * metrics.cell_h;
        let bar = (metrics.cell_w / 8.0).max(1.0);
        let (pos, size) = match cursor.shape {
            // Bar and underline sit next to the glyph, so the glyph keeps its colour.
            2 => ([left, top], [bar, metrics.cell_h]),
            1 => ([left, top + metrics.cell_h - bar], [metrics.cell_w, bar]),
            // Block: the rect went in above the glyph (rects draw first, then glyphs).
            _ => ([left, top], [metrics.cell_w, metrics.cell_h]),
        };
        batch.rects.push(RectInstance {
            pos,
            size,
            color: Palette::components(palette.foreground),
        });
    }

    batch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_layout_matches_the_shader() {
        assert_eq!(
            std::mem::size_of::<RectInstance>(),
            RectInstance::STRIDE as usize
        );
        assert_eq!(
            std::mem::size_of::<GlyphInstance>(),
            GlyphInstance::STRIDE as usize
        );
    }
}
