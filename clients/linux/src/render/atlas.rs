//! The glyph atlases: an R8 mask atlas for ordinary text and an RGBA atlas for colour
//! glyphs (emoji), so text never pays four bytes per texel.
//!
//! One glyph is rasterized per `(face, glyph, bold)` on first use, packed by shelf, uploaded
//! once. Rasterization is swash; the colour paths (COLR outlines, CBDT/CBLC bitmaps) come
//! from the same call with a different source list.

use std::collections::HashMap;

use glow::HasContext;
use swash::scale::{Render, ScaleContext, Source, StrikeWith};
use swash::zeno::Format;

use super::font::Font;

/// Atlas edge in device pixels. 2048² of 8-bit coverage is 4 MiB and holds a screen's worth
/// of glyphs many times over; the colour atlas is the same size at 4 bytes per texel.
pub const SIDE: i32 = 2048;

const SOURCES: &[Source] = &[
    // A COLR font (emoji, some icon fonts) draws the glyph itself, in colour.
    Source::ColorOutline(0),
    // CBDT/CBLC and sbix strikes: emoji on most Linux systems.
    Source::ColorBitmap(StrikeWith::BestFit),
    Source::Outline,
];

/// Which atlas a slot lives in; the renderer picks the sampler and the blend mode from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// 8-bit coverage, tinted by the cell's foreground colour.
    Mask,
    /// Straight-premultiplied RGBA, drawn as is.
    Color,
}

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
    pub layer: Layer,
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

/// Shelf packing state for one atlas.
#[derive(Default)]
struct Pack {
    x: i32,
    y: i32,
    row: i32,
}

pub struct Atlas {
    pub mask: glow::Texture,
    pub color: glow::Texture,
    context: ScaleContext,
    /// `None` remembers a glyph that rasterizes to nothing, so it is not retried per frame.
    slots: HashMap<Key, Option<Slot>>,
    mask_pack: Pack,
    color_pack: Pack,
    /// Bumped whenever an atlas is emptied; the caller rebuilds the frame afterwards.
    pub generation: u32,
}

impl Atlas {
    pub fn new(gl: &glow::Context) -> Result<Self, String> {
        let mask = texture(gl, glow::R8, glow::RED)?;
        let color = texture(gl, glow::RGBA8, glow::RGBA)?;
        Ok(Self {
            mask,
            color,
            context: ScaleContext::new(),
            slots: HashMap::new(),
            mask_pack: Pack::default(),
            color_pack: Pack::default(),
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
        let Some(slot) = self.intern(gl, key, &image) else {
            // Full: start over. The caller rebuilds the frame against the empty atlas.
            self.clear();
            let slot = self.intern(gl, key, &image)?;
            return Some(slot);
        };
        Some(slot)
    }

    /// Empties both atlases and invalidates every slot the caller holds.
    pub fn clear(&mut self) {
        self.slots.clear();
        self.mask_pack = Pack::default();
        self.color_pack = Pack::default();
        self.generation = self.generation.wrapping_add(1);
    }

    fn intern(&mut self, gl: &glow::Context, key: Key, image: &Raster) -> Option<Slot> {
        let pack = match image.layer {
            Layer::Mask => &mut self.mask_pack,
            Layer::Color => &mut self.color_pack,
        };
        let (x, y) = allocate(pack, image.width, image.height)?;
        let texture = match image.layer {
            Layer::Mask => self.mask,
            Layer::Color => self.color,
        };
        let format = match image.layer {
            Layer::Mask => glow::RED,
            Layer::Color => glow::RGBA,
        };
        unsafe {
            gl.bind_texture(glow::TEXTURE_2D, Some(texture));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 1);
            gl.tex_sub_image_2d(
                glow::TEXTURE_2D,
                0,
                x,
                y,
                image.width,
                image.height,
                format,
                glow::UNSIGNED_BYTE,
                glow::PixelUnpackData::Slice(Some(&image.data)),
            );
            gl.bind_texture(glow::TEXTURE_2D, None);
        }
        let slot = Slot {
            x,
            y,
            w: image.width,
            h: image.height,
            left: image.left,
            top: image.top,
            layer: image.layer,
        };
        self.slots.insert(key, Some(slot));
        Some(slot)
    }

    fn rasterize(&mut self, font: &Font, key: Key) -> Option<Raster> {
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
        Some(Raster {
            data: image.data,
            width: image.placement.width as i32,
            height: image.placement.height as i32,
            left: image.placement.left,
            top: image.placement.top,
            layer: match image.content {
                swash::scale::image::Content::Color => Layer::Color,
                _ => Layer::Mask,
            },
        })
    }
}

fn texture(gl: &glow::Context, internal: u32, format: u32) -> Result<glow::Texture, String> {
    let texture = unsafe { gl.create_texture() }.map_err(|error| format!("texture: {error}"))?;
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
            internal as i32,
            SIDE,
            SIDE,
            0,
            format,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        gl.bind_texture(glow::TEXTURE_2D, None);
    }
    Ok(texture)
}

/// Shelf packing: fill a row left to right, then start the next row under the tallest one.
fn allocate(pack: &mut Pack, width: i32, height: i32) -> Option<(i32, i32)> {
    // One pixel of padding keeps linear sampling off the neighbour.
    let w = width + 1;
    let h = height + 1;
    if w > SIDE {
        return None;
    }
    if pack.x + w > SIDE {
        pack.x = 0;
        pack.y += pack.row;
        pack.row = 0;
    }
    if pack.y + h > SIDE {
        return None;
    }
    let origin = (pack.x, pack.y);
    pack.x += w;
    pack.row = pack.row.max(h);
    Some(origin)
}

struct Raster {
    data: Vec<u8>,
    width: i32,
    height: i32,
    left: i32,
    top: i32,
    layer: Layer,
}
