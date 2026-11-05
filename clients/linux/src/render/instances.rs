//! Turning one screen into the instance batches the shaders eat.
//!
//! Field order and sizes must match the GLSL in `gl.rs` (`docs/renderer.md` §5.3).

use super::atlas::{Atlas, Key, Layer};
use super::boxdraw;
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
    /// Glyphs from the 8-bit atlas, tinted by `color`.
    pub glyphs: Vec<GlyphInstance>,
    /// Colour glyphs (emoji) from the RGBA atlas, drawn premultiplied.
    pub color_glyphs: Vec<GlyphInstance>,
}

/// Everything a frame needs besides the screens themselves; one value is built per frame so
/// the per-pane call stays short.
pub struct Draw<'a> {
    pub font: &'a Font,
    pub metrics: GridMetrics,
    pub palette: &'a Palette,
    pub atlas: &'a mut Atlas,
    pub shaped: &'a mut super::shaped_cell::ShapedCell,
}

/// Draws every cell of `screen` at `origin` plus the cursor.
///
/// The whole grid is rebuilt each frame; dirty-row tracking is the next optimisation, and
/// is worth doing only once `PIGEON_FRAME_STATS=1` says it matters (`docs/renderer.md` §5.4).
pub fn build(draw: &mut Draw<'_>, screen: &Screen, origin: [f32; 2], cursor_on: bool) -> Batch {
    let Draw {
        font,
        metrics,
        palette,
        atlas,
        shaped,
    } = draw;
    let metrics = *metrics;
    let [origin_x, origin_y] = origin;
    let cols = screen.cols();
    let rows = screen.rows();
    let mut batch = Batch::default();
    let cursor = &screen.snapshot.cursor;
    let cursor_cell = (usize::from(cursor.x), usize::from(cursor.y));

    for y in 0..rows {
        let top = origin_y + y as f32 * metrics.cell_h;
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
            let left = origin_x + x as f32 * metrics.cell_w;
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
            let mut text = Palette::components(glyph_color);
            if attrs & pigeon_core::api::ATTR_DIM != 0 {
                text[3] *= 0.6;
            }

            // Box drawing and blocks are rectangles: a font's `─` rarely meets its
            // neighbour, and TUI borders then break into dashes.
            if cell.extra_text.is_none()
                && boxdraw::handles(cell.codepoint)
                && boxdraw::draw(
                    cell.codepoint,
                    [left, top],
                    [width, metrics.cell_h],
                    metrics.line_thickness,
                    text,
                    &mut batch.rects,
                )
            {
                continue;
            }

            if let Some(value) = &cell.extra_text {
                for shaped in shaped.get(font, value).iter() {
                    let key = Key {
                        face: shaped.face,
                        glyph: shaped.glyph,
                        bold: attrs & pigeon_core::api::ATTR_BOLD != 0,
                    };
                    if let Some(slot) = atlas.slot(font, key) {
                        let instance = GlyphInstance {
                            pos: [
                                left + shaped.x + slot.left as f32,
                                top + metrics.baseline - shaped.y - slot.top as f32,
                            ],
                            size: [slot.w as f32, slot.h as f32],
                            uv: slot.uv(),
                            color: text,
                        };
                        match slot.layer {
                            Layer::Mask => batch.glyphs.push(instance),
                            Layer::Color => batch.color_glyphs.push(instance),
                        }
                    }
                }
            } else if cell.codepoint > 0x20 && cell.codepoint != 0x7f {
                if let Some(ch) = char::from_u32(cell.codepoint) {
                    if let Some((face, glyph)) = font.glyph(ch) {
                        let key = Key {
                            face,
                            glyph,
                            bold: attrs & pigeon_core::api::ATTR_BOLD != 0,
                        };
                        if let Some(slot) = atlas.slot(font, key) {
                            let baseline = top + metrics.baseline;
                            // A wide cell is two cells; a fallback face's glyph is often
                            // narrower than that, so centre it instead of stretching it.
                            let inset = if cell.width < 2 || slot.w as f32 > width {
                                0.0
                            } else {
                                (width - slot.w as f32) / 2.0
                            };
                            let instance = GlyphInstance {
                                pos: [
                                    (left + inset + slot.left as f32).round(),
                                    baseline - slot.top as f32,
                                ],
                                size: [slot.w as f32, slot.h as f32],
                                uv: slot.uv(),
                                color: text,
                            };
                            match slot.layer {
                                Layer::Mask => batch.glyphs.push(instance),
                                Layer::Color => batch.color_glyphs.push(instance),
                            }
                        }
                    }
                }
            }

            if let Some(attr) = decoration(attrs) {
                let thickness = metrics.line_thickness;
                let y_offset = if attr == pigeon_core::api::ATTR_UNDERLINE {
                    metrics.baseline + metrics.line_thickness
                } else {
                    metrics.baseline - metrics.cell_h * 0.25
                };
                batch.rects.push(RectInstance {
                    pos: [left, top + y_offset],
                    size: [width, thickness],
                    color: text,
                });
            }
        }
    }

    if cursor_on && cursor.visible {
        let (cx, cy) = cursor_cell;
        let left = origin_x + cx as f32 * metrics.cell_w;
        let top = origin_y + cy as f32 * metrics.cell_h;
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

mod sealed {
    pub trait Instance {}
}
/// Only the two padding-free repr(C) float layouts can be uploaded as raw bytes.
pub trait Instance: sealed::Instance {}
impl sealed::Instance for RectInstance {}
impl Instance for RectInstance {}
impl sealed::Instance for GlyphInstance {}
impl Instance for GlyphInstance {}
const _: [(); 32] = [(); std::mem::size_of::<RectInstance>()];
const _: [(); 48] = [(); std::mem::size_of::<GlyphInstance>()];
