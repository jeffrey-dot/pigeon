//! The Linux grid renderer: one OpenGL pass over two instance batches.
//!
//! Shape follows Ghostty's generic renderer and the macOS client's Metal path — cells in,
//! quads out — but there is only one backend here, so the "generic over the graphics API"
//! split waits until a second backend exists ([adr/0011](../../../../docs/adr/0011-linux-rendering.md)).

mod atlas;
mod boxdraw;
mod current_context;
mod device;
mod font;
#[allow(unsafe_code)]
mod gl;
mod instances;
mod palette;
mod pipeline;
mod shaders;
mod shaped_cell;
mod texture;

use std::rc::Rc;

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

/// One pane in a frame: where its grid starts, how much room it has, what to draw.
pub struct PaneFrame<'a> {
    /// Device pixels from the canvas top-left to the pane's top-left.
    pub origin: [f32; 2],
    pub size: [f32; 2],
    pub screen: &'a Screen,
    /// The cursor is only drawn for the pane the keyboard goes to.
    pub focused: bool,
}

pub struct Renderer {
    gl: Rc<gl::Gl>,
    rects: gl::Pipeline,
    glyphs: gl::Pipeline,
    atlas: Atlas,
    font: Font,
    shaped: shaped_cell::ShapedCell,
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
        let dialect = shaders::Dialect::detect(&gl.version());
        if std::env::var_os("PIGEON_FRAME_STATS").is_some() {
            eprintln!(
                "pigeon: renderer on {} ({})",
                gl.version(),
                dialect.describe()
            );
        }
        let [rect_vs, rect_fs, glyph_vs, glyph_fs] = shaders::sources(dialect);
        let rects = gl::pipeline(
            &gl,
            &rect_vs,
            &rect_fs,
            &[(0, 2, 0), (1, 2, 8), (2, 4, 16)],
            instances::RectInstance::STRIDE,
        )?;
        let glyphs = gl::pipeline(
            &gl,
            &glyph_vs,
            &glyph_fs,
            &[(0, 2, 0), (1, 2, 8), (2, 4, 16), (3, 4, 32)],
            instances::GlyphInstance::STRIDE,
        )?;
        let atlas = Atlas::new(&gl)?;
        let font = Font::load(FONT_POINTS * scale)?;
        Ok(Self {
            gl,
            rects,
            glyphs,
            atlas,
            font,
            shaped: shaped_cell::ShapedCell::default(),
            palette: Palette::dark(),
            scale,
            frame: 0,
            stats_at: std::time::Instant::now(),
        })
    }

    pub fn owns_current_context(&self) -> bool {
        self.gl.owns_current_context()
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.palette = palette;
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

    /// Draws every pane into the current framebuffer, `width`×`height` device pixels (what
    /// GTK gives the GLArea: its allocation times the scale factor).
    pub fn draw(
        &mut self,
        panes: &[PaneFrame<'_>],
        dividers: &[[f32; 4]],
        width: i32,
        height: i32,
    ) {
        if width <= 0 || height <= 0 {
            return;
        }
        let started = std::time::Instant::now();
        let (mut rects, mut glyphs, mut colour_glyphs) = (0, 0, 0);
        self.gl.begin((width, height), self.background());

        {
            let mut draw = instances::Draw {
                font: &self.font,
                metrics: self.font.metrics,
                palette: &self.palette,
                atlas: &mut self.atlas,
                shaped: &mut self.shaped,
            };
            for pane in panes {
                let [x, y] = pane.origin;
                let [w, h] = pane.size;
                let batch = instances::build(&mut draw, pane.screen, pane.origin, pane.focused);
                // Uploading a new glyph changes texture bindings; bind the finished atlases.
                self.glyphs
                    .bind_atlases(&draw.atlas.mask, &draw.atlas.color);
                self.gl.scissor(Some([
                    x as i32,
                    height - y as i32 - h as i32,
                    w as i32,
                    h as i32,
                ]));
                self.rects.draw(&batch.rects, (width, height));
                self.glyphs.set_kind(0);
                self.glyphs.draw(&batch.glyphs, (width, height));
                self.glyphs.set_kind(1);
                self.glyphs.draw(&batch.color_glyphs, (width, height));
                self.glyphs.set_kind(0);
                rects += batch.rects.len();
                glyphs += batch.glyphs.len();
                colour_glyphs += batch.color_glyphs.len();
            }
        }
        if !dividers.is_empty() {
            // A muted line, not a full-strength rule: the pane content is the subject.
            let mut colour = Palette::components(self.palette.foreground);
            colour[3] = 0.18;
            let lines: Vec<instances::RectInstance> = dividers
                .iter()
                .map(|line| instances::RectInstance {
                    pos: [line[0], line[1]],
                    size: [line[2], line[3]],
                    color: colour,
                })
                .collect();
            self.gl.scissor(None);
            self.rects.draw(&lines, (width, height));
        }
        self.gl.finish();
        self.record(started, rects, glyphs + colour_glyphs);
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
