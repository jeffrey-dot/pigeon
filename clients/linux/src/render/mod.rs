//! The Linux grid renderer: one OpenGL pass over two instance batches.
//!
//! Shape follows Ghostty's generic renderer and the macOS client's Metal path — cells in,
//! quads out — but there is only one backend here, so the "generic over the graphics API"
//! split waits until a second backend exists ([adr/0011](../../../../docs/adr/0011-linux-rendering.md)).

mod atlas;
mod font;
mod gl;
mod instances;
mod palette;

use glow::HasContext;

pub use self::font::GridMetrics;
pub use self::palette::Palette;

use self::atlas::Atlas;
use self::font::Font;
use crate::screen::Screen;

/// Default point size; the theme file that would carry it does not exist on this side yet.
pub const FONT_POINTS: f32 = 13.0;

/// Cell geometry without a GL context, for sizing the window before the first frame.
pub fn measure(scale: f32) -> Option<GridMetrics> {
    font::measure(FONT_POINTS * scale)
}

pub struct Renderer {
    gl: gl::Gl,
    rects: gl::Pipeline,
    glyphs: gl::Pipeline,
    atlas: Atlas,
    font: Font,
    palette: Palette,
    /// Device pixels per logical pixel the font was loaded for.
    scale: f32,
    /// Frame counter for `PIGEON_FRAME_STATS=1`.
    frame: u64,
    stats_at: std::time::Instant,
}

impl Renderer {
    /// Needs the GLArea's context to be current.
    pub fn new(scale: f32) -> Result<Self, String> {
        let gl = gl::Gl::load()?;
        let context = &gl.context;
        let dialect = gl::Dialect::detect(context);
        if std::env::var_os("PIGEON_FRAME_STATS").is_some() {
            eprintln!(
                "pigeon: renderer on {} ({})",
                gl.version(),
                dialect.describe()
            );
        }
        let [rect_vs, rect_fs, glyph_vs, glyph_fs] = gl::sources(dialect);
        let rects = gl::pipeline(
            context,
            &rect_vs,
            &rect_fs,
            &[(0, 2, 0), (1, 2, 8), (2, 4, 16)],
            instances::RectInstance::STRIDE,
        )?;
        let glyphs = gl::pipeline(
            context,
            &glyph_vs,
            &glyph_fs,
            &[(0, 2, 0), (1, 2, 8), (2, 4, 16), (3, 4, 32)],
            instances::GlyphInstance::STRIDE,
        )?;
        let atlas = Atlas::new(context)?;
        let font = Font::load(FONT_POINTS * scale)?;
        unsafe {
            context.enable(glow::BLEND);
            context.blend_func(glow::SRC_ALPHA, glow::ONE_MINUS_SRC_ALPHA);
        }
        Ok(Self {
            gl,
            rects,
            glyphs,
            atlas,
            font,
            palette: Palette::dark(),
            scale,
            frame: 0,
            stats_at: std::time::Instant::now(),
        })
    }

    pub fn metrics(&self) -> GridMetrics {
        self.font.metrics
    }

    pub fn background(&self) -> [f32; 4] {
        Palette::components(self.palette.background)
    }

    /// Reloads the font when the window moves to a display with another scale factor.
    pub fn set_scale(&mut self, scale: f32) {
        if (scale - self.scale).abs() < f32::EPSILON {
            return;
        }
        self.scale = scale;
        match Font::load(FONT_POINTS * scale) {
            Ok(font) => {
                self.font = font;
                self.atlas.clear();
            }
            Err(error) => eprintln!("pigeon: keeping the old font: {error}"),
        }
    }

    /// Draws one full frame into the current framebuffer, `width`×`height` device pixels
    /// (what GTK gives the GLArea: its allocation times the scale factor).
    pub fn draw(&mut self, screen: &Screen, cursor_on: bool, width: i32, height: i32) {
        let context = &self.gl.context;
        if width <= 0 || height <= 0 {
            return;
        }
        let started = std::time::Instant::now();
        let batch = instances::build(
            context,
            screen,
            &self.font,
            self.font.metrics,
            &self.palette,
            &mut self.atlas,
            cursor_on,
        );
        unsafe {
            context.viewport(0, 0, width, height);
            let clear = self.background();
            context.clear_color(clear[0], clear[1], clear[2], clear[3]);
            context.clear(glow::COLOR_BUFFER_BIT);
            self.rects.draw(context, &batch.rects, (width, height));
            context.active_texture(glow::TEXTURE0);
            context.bind_texture(glow::TEXTURE_2D, Some(self.atlas.texture));
            self.glyphs.bind_atlas(context);
            self.glyphs.draw(context, &batch.glyphs, (width, height));
            context.bind_texture(glow::TEXTURE_2D, None);
        }
        self.record(started, batch.rects.len(), batch.glyphs.len());
    }

    /// `PIGEON_FRAME_STATS=1` prints one line a second: the same knob the macOS client has,
    /// so the two can be compared without a profiler.
    fn record(&mut self, started: std::time::Instant, rects: usize, glyphs: usize) {
        self.frame += 1;
        if std::env::var_os("PIGEON_FRAME_STATS").is_none() {
            return;
        }
        let elapsed = self.stats_at.elapsed();
        if elapsed.as_secs_f32() < 1.0 {
            return;
        }
        eprintln!(
            "pigeon: {} frames/s cpu={:.2}ms rects={} glyphs={} atlas={}",
            self.frame as f32 / elapsed.as_secs_f32(),
            started.elapsed().as_secs_f64() * 1000.0,
            rects,
            glyphs,
            self.atlas.generation,
        );
        self.frame = 0;
        self.stats_at = std::time::Instant::now();
    }
}
