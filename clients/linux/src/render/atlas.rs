//! The R8 glyph atlas: rasterize a glyph with swash the first time a frame asks for it,
//! pack it by shelf, upload one rectangle.
//!
//! One 8-bit mask atlas is enough here: colour glyphs (emoji) need the BGRA atlas the macOS
//! client has, and are a later step (`docs/renderer.md` §5.4).

use std::collections::HashMap;

use glow::HasContext;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;

use super::font::Font;

/// Atlas edge in device pixels. 2048² of 8-bit coverage is 4 MiB and holds a screen's worth
/// of glyphs many times over.
pub const SIDE: i32 = 2048;

const SOURCES: &[Source] = &[Source::Outline];

/// Where one glyph lives, plus how to place it relative to the cell's baseline.
#[derive(Debug, Clone, Copy)]
pub struct Slot {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// Horizontal offset from the cell's left edge to the image.
    pub left: i32,
    /// Height of the image above the baseline (y-up, as swash reports it).
    pub top: i32,
}

impl Slot {
    pub fn uv(&self) -> [f32; 4] {
        let side = SIDE as f32;
        [
            self.x as f32 / side,
            self.y as f32 / side,
            self.w as f32 / side,
            self.h as f32 / side,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    /// Which loaded face holds the glyph (`Font::glyph`).
    pub face: u8,
    pub glyph: u16,
    /// Synthetic bold: the same glyph rasterized with a thicker outline.
    pub bold: bool,
}

pub struct Atlas {
    pub texture: glow::Texture,
    context: ScaleContext,
    /// `None` remembers a glyph that rasterizes to nothing, so it is not retried per frame.
    slots: HashMap<Key, Option<Slot>>,
    x: i32,
    y: i32,
    row: i32,
    /// Bumped whenever the atlas is emptied; the frame builder uses it to drop stale slots.
    pub generation: u32,
}

impl Atlas {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        let texture =
            unsafe { gl.create_texture() }.map_err(|error| format!("texture: {error}"))?;
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MIN_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_MAG_FILTER,
                glow::LINEAR as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_S,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_parameter_i32(
                glow::TEXTURE_2D,
                glow::TEXTURE_WRAP_T,
                glow::CLAMP_TO_EDGE as i32,
            );
            gl.tex_image_2d(
                glow::TEXTURE_2D,
                0,
                glow::R8 as i32,
                SIDE,
                SIDE,
                0,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(None),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        Ok(Self {
            texture,
            context: ScaleContext::new(),
            slots: HashMap::new(),
            x: 0,
            y: 0,
            row: 0,
            generation: 0,
        })
    }

    /// The slot for `key`, rasterizing and uploading it on first use. `None` means "draw
    /// nothing here" (a blank glyph, or a full atlas that was just cleared).
    pub fn slot(&mut self, gl: &glow::Context, font: &Font, key: Key) -> Option<Slot> {
        if let Some(hit) = self.slots.get(&key) {
            return *hit;
        }
        let Some(image) = self.rasterize(font, key) else {
            // Remember the blank so a screen full of missing codepoints is not retried
            // every frame.
            self.slots.insert(key, None);
            return None;
        };
        let Some((x, y)) = self.allocate(image.width, image.height) else {
            // Full: start over. The caller rebuilds the frame against the empty atlas.
            self.clear();
            let allocated = self.allocate(image.width, image.height)?;
            self.upload(gl, allocated, &image);
            let slot = Slot {
                x: allocated.0,
                y: allocated.1,
                w: image.width,
                h: image.height,
                left: image.left,
                top: image.top,
            };
            self.slots.insert(key, Some(slot));
            return Some(slot);
        };
        self.upload(gl, (x, y), &image);
        let slot = Slot {
            x,
            y,
            w: image.width,
            h: image.height,
            left: image.left,
            top: image.top,
        };
        self.slots.insert(key, Some(slot));
        Some(slot)
    }

    /// Empties the atlas and invalidates every slot the caller holds.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.x = 0;
        self.y = 0;
        self.row = 0;
        self.generation = self.generation.wrapping_add(1);
    }

    fn rasterize(&mut self, font: &Font, key: Key) -> Option<Mask> {
        let font_ref = font.face(key.face)?;
        let mut scaler = self
            .context
            .builder(font_ref)
            .size(font.size)
            .hint(true)
            .build();
        let mut render = Render::new(SOURCES);
        render.format(Format::Alpha);
        if key.bold {
            render.embolden((font.size * 0.035).max(0.6));
        }
        let image = render.render(&mut scaler, key.glyph)?;
        if image.placement.width == 0 || image.placement.height == 0 {
            return None;
        }
        Some(Mask {
            data: image.data,
            width: image.placement.width as i32,
            height: image.placement.height as i32,
            left: image.placement.left,
            top: image.placement.top,
        })
    }

    fn allocate(&mut self, width: i32, height: i32) -> Option<(i32, i32)> {
        // One pixel of padding keeps linear sampling off the neighbour.
        let w = width + 1;
        let h = height + 1;
        if w > SIDE {
            return None;
        }
        if self.x + w > SIDE {
            self.x = 0;
            self.y += self.row;
            self.row = 0;
        }
        if self.y + h > SIDE {
            return None;
        }
        let origin = (self.x, self.y);
        self.x += w;
        self.row = self.row.max(h);
        Some(origin)
    }

    fn upload(&mut self, gl: &glow::Context, origin: (i32, i32), mask: &Mask) {
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(self.texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                origin.0,
                origin.1,
                mask.width,
                mask.height,
                glow::RED,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&mask.data)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
    }
}

struct Mask {
    data: Vec<u8>,
    width: i32,
    height: i32,
    left: i32,
    top: i32,
}
