//! Font discovery (fontdb) and grid metrics (swash).
//!
//! A terminal wants one glyph per cell, not rich text, so this deliberately does not go
//! through Pango: fontdb finds the monospace face, swash gives metrics and rasterization
//! (`docs/renderer.md` §5.1).

use fontdb::{Database, Family, Query, Style, Weight};
use swash::FontRef;

/// Device-pixel geometry of one cell.
#[derive(Debug, Clone, Copy)]
pub struct GridMetrics {
    pub cell_w: f32,
    pub cell_h: f32,
    /// Distance from the top of the cell to the baseline.
    pub baseline: f32,
}

/// One loaded face. The bytes are leaked on purpose: `FontRef` borrows them and lives as
/// long as the renderer does.
struct Face {
    data: &'static [u8],
    index: u32,
}

impl Face {
    fn font_ref(&self) -> Option<FontRef<'static>> {
        FontRef::from_index(self.data, self.index as usize)
    }
}

pub struct Font {
    /// `faces[0]` is the cell face; the rest fill in codepoints it does not have (CJK,
    /// box drawing, symbols). Metrics always come from `faces[0]`, or wide text would
    /// reflow the grid.
    faces: Vec<Face>,
    /// Device pixels per em (point size × scale factor).
    pub size: f32,
    pub metrics: GridMetrics,
}

/// Faces tried after the cell face, in order, for codepoints it lacks.
const FALLBACKS: &[&str] = &[
    "Noto Sans Mono CJK SC",
    "Noto Sans CJK SC",
    "Source Han Sans SC",
    "Source Han Mono SC",
    "Sarasa Mono SC",
    "WenQuanYi Zen Hei",
    "Noto Sans Mono",
    "DejaVu Sans",
    "Symbols Nerd Font Mono",
];

impl Font {
    pub fn load(size: f32) -> Result<Self, String> {
        let (data, index) = monospace()?;
        let data: &'static [u8] = Box::leak(data.into_boxed_slice());
        let primary = Face { data, index };
        let font_ref = primary
            .font_ref()
            .ok_or_else(|| "font face does not parse".to_owned())?;
        let mut faces = vec![primary];
        for name in FALLBACKS {
            if let Some((bytes, index)) = by_name(name) {
                let data: &'static [u8] = Box::leak(bytes.into_boxed_slice());
                faces.push(Face { data, index });
            }
        }
        Ok(Self {
            faces,
            size,
            metrics: measure_of(font_ref, size),
        })
    }

    /// `None` only if the face stopped parsing, which `load` already ruled out; callers
    /// skip drawing rather than panic, because a paint path must not take the window down.
    pub fn face(&self, index: u8) -> Option<FontRef<'static>> {
        self.faces.get(usize::from(index))?.font_ref()
    }

    /// Which face draws `codepoint`, and the glyph id in it. `None` means no loaded face
    /// has it, so the cell stays blank instead of showing a wrong glyph.
    pub fn glyph(&self, codepoint: char) -> Option<(u8, u16)> {
        for (index, face) in self.faces.iter().enumerate() {
            let Some(font) = face.font_ref() else {
                continue;
            };
            let glyph = font.charmap().map(codepoint);
            if glyph != 0 {
                return u8::try_from(index).ok().map(|index| (index, glyph));
            }
        }
        None
    }
}

/// Cell geometry without keeping the face: what the window needs before any GL context
/// exists, so it can open at the size the host's screen already has.
pub fn measure(size: f32) -> Option<GridMetrics> {
    let (data, index) = monospace().ok()?;
    let font_ref = FontRef::from_index(&data, index as usize)?;
    Some(measure_of(font_ref, size))
}

/// Fonts we would rather have than whatever the system calls "monospace": `fontdb`'s
/// generic family resolves to Nimbus Mono on a bare Arch install, which is a Courier clone
/// with 0.6 em ascent and no programming ligatures. `PIGEON_FONT` overrides the list.
const PREFERRED: &[&str] = &[
    "JetBrains Mono",
    "JetBrainsMono Nerd Font Mono",
    "Fira Code",
    "Cascadia Code",
    "Source Code Pro",
    "Hack",
    "DejaVu Sans Mono",
    "Liberation Mono",
    "Noto Sans Mono",
];

fn monospace() -> Result<(Vec<u8>, u32), String> {
    let database = system_fonts();
    let requested = std::env::var("PIGEON_FONT").ok();
    let names: Vec<&str> = match &requested {
        Some(name) => vec![name.as_str()],
        None => PREFERRED.to_vec(),
    };
    for name in names {
        if let Some((bytes, index)) = query(&database, &[Family::Name(name)]) {
            return Ok((bytes, index));
        }
    }
    query(&database, &[Family::Monospace]).ok_or_else(|| "no monospace font installed".to_owned())
}

/// A named family, ignoring weight and style: see `FALLBACKS`.
fn by_name(name: &str) -> Option<(Vec<u8>, u32)> {
    let database = system_fonts();
    query(&database, &[Family::Name(name)])
}

fn system_fonts() -> Database {
    let mut database = Database::new();
    database.load_system_fonts();
    database
}

fn query(database: &Database, families: &[Family<'_>]) -> Option<(Vec<u8>, u32)> {
    let query = Query {
        families,
        weight: Weight::NORMAL,
        style: Style::Normal,
        ..Query::default()
    };
    load(database, &query)
}

fn load(database: &Database, query: &Query<'_>) -> Option<(Vec<u8>, u32)> {
    let id = database.query(query)?;
    if std::env::var_os("PIGEON_FONT_DEBUG").is_some() {
        if let Some(face) = database.face(id) {
            eprintln!(
                "pigeon: font {:?} weight {:?}",
                face.families.first().map(|(name, _)| name.as_str()),
                face.weight
            );
        }
    }
    database.with_face_data(id, |data, index| (data.to_vec(), index))
}

fn measure_of(font_ref: FontRef<'_>, size: f32) -> GridMetrics {
    let metrics = font_ref.metrics(&[]).scale(size);
    let charmap = font_ref.charmap();
    let advances = font_ref.glyph_metrics(&[]).scale(size);
    let advance = advances.advance_width(charmap.map('M'));
    let advance = if advance > 0.0 {
        advance
    } else {
        metrics.average_width
    };
    let line =
        (metrics.ascent + metrics.descent + metrics.leading).max(metrics.ascent + metrics.descent);
    GridMetrics {
        cell_w: advance.max(1.0).round(),
        cell_h: line.max(1.0).round(),
        baseline: metrics.ascent.round(),
    }
}
