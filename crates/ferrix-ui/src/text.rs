//! The shared text path: IBM Plex faces, Noto fallback, shaping, bidi, and
//! software glyph rasterisation for every Ferrix surface (issue #14).
//!
//! - **Faces.** IBM Plex Sans Condensed (Regular, SemiBold) for chrome and
//!   body text and IBM Plex Mono (Regular, Bold) for terminal cells and code
//!   are compiled in from the pinned, hash-checked files under
//!   `assets/fonts/ibm-plex/` (SIL OFL 1.1; see `build.rs` and
//!   `docs/licensing.md`). They always load, so Latin text never depends on
//!   the guest.
//! - **Fallback.** Scripts Plex does not cover (Arabic, Devanagari, CJK,
//!   Cyrillic beyond Plex, symbols) resolve to the Noto families installed in
//!   the guest under `/usr/share/fonts` (`FERRIX_FONT_DIRS`, colon-separated,
//!   overrides the directories). Those directories are scanned only when a
//!   string first needs a glyph the embedded faces lack, so a Latin-only
//!   process never pays for the scan. The order is the project-owned
//!   [`NotoFallback`], identical on every host, not the platform's list.
//! - **Shaping and bidi.** `cosmic-text` (HarfRust shaping, `unicode-bidi`
//!   reordering) lays out each line; Arabic joins, Devanagari forms
//!   conjuncts, and mixed RTL/LTR runs are reordered visually.
//! - **Rasterisation.** `swash` renders coverage masks (and colour bitmaps,
//!   for a colour emoji face when one is installed) on the CPU. Shaped lines
//!   and their rasters are cached per (text, face, weight, size), so redrawing
//!   an unchanged frame does no shaping; that keeps TCG emulation cheap.
//!
//! A glyph no loaded face has is drawn as the face's missing-glyph box
//! (tofu), honestly, never silently dropped; [`Type::has_glyphs`] lets a
//! caller choose a substitute first.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use cosmic_text::fontdb;
use cosmic_text::{
    Attrs, Buffer, CacheKey, Fallback, Family, FontSystem, Metrics, Shaping, SwashCache,
    SwashContent, Wrap,
};
use unicode_script::Script;

use crate::draw::Canvas;

/// The two Ferrix type families. `Sans` is the system voice; `Mono` is for
/// terminal cells and source text, never proportional text in a grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Face {
    #[default]
    Sans,
    Mono,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    /// SemiBold in Plex Sans Condensed (titles, readouts); Bold in Plex Mono.
    Bold,
}

#[derive(Clone, Copy, Debug)]
pub struct Style {
    pub weight: Weight,
    pub px: f32,
    pub color: u32,
    pub face: Face,
}

impl Style {
    pub fn regular(px: f32, color: u32) -> Self {
        Self {
            weight: Weight::Regular,
            px,
            color,
            face: Face::Sans,
        }
    }

    pub fn bold(px: f32, color: u32) -> Self {
        Self {
            weight: Weight::Bold,
            ..Self::regular(px, color)
        }
    }

    pub fn mono(px: f32, color: u32) -> Self {
        Self {
            face: Face::Mono,
            ..Self::regular(px, color)
        }
    }

    pub fn mono_bold(px: f32, color: u32) -> Self {
        Self {
            face: Face::Mono,
            ..Self::bold(px, color)
        }
    }

    /// The same style in another family.
    pub fn with_face(self, face: Face) -> Self {
        Self { face, ..self }
    }
}

const SANS_FAMILY: &str = "IBM Plex Sans Condensed";
const MONO_FAMILY: &str = "IBM Plex Mono";

/// The embedded faces, verified against `assets/fonts/ibm-plex/SHA256SUMS`
/// by `build.rs` before they can be compiled in.
static EMBEDDED: [(Face, Weight, &[u8]); 4] = [
    (
        Face::Sans,
        Weight::Regular,
        include_bytes!("../assets/fonts/ibm-plex/IBMPlexSansCondensed-Regular.ttf"),
    ),
    (
        Face::Sans,
        Weight::Bold,
        include_bytes!("../assets/fonts/ibm-plex/IBMPlexSansCondensed-SemiBold.ttf"),
    ),
    (
        Face::Mono,
        Weight::Regular,
        include_bytes!("../assets/fonts/ibm-plex/IBMPlexMono-Regular.ttf"),
    ),
    (
        Face::Mono,
        Weight::Bold,
        include_bytes!("../assets/fonts/ibm-plex/IBMPlexMono-Bold.ttf"),
    ),
];

/// The IBM Plex OFL 1.1 notice, shipped with every binary that embeds the
/// faces (Settings > About names it).
pub const IBM_PLEX_LICENSE: &str = include_str!("../assets/fonts/ibm-plex/LICENSE.txt");

/// One-line attribution for About pages and notices.
pub const ATTRIBUTION: &str = "Type: IBM Plex and Noto, SIL OFL 1.1";

/// Guest font directories scanned for fallback faces.
const DEFAULT_FONT_DIRS: &[&str] = &["/usr/share/fonts"];

/// Shaped lines kept per process before the cache is cleared. A frame draws
/// a few hundred distinct strings at most; a terminal grid hits the same
/// few hundred single characters.
const LINE_CACHE_LIMIT: usize = 4096;

/// Project-owned fallback order: the Noto families by script, the same on
/// every host (cosmic-text's platform lists differ per OS, which would make
/// host previews and tests disagree with the guest).
struct NotoFallback;

fn han(locale: &str) -> &'static [&'static str] {
    match locale {
        l if l.starts_with("ja") => &["Noto Sans CJK JP", "Noto Sans CJK SC"],
        l if l.starts_with("ko") => &["Noto Sans CJK KR", "Noto Sans CJK SC"],
        "zh-HK" => &["Noto Sans CJK HK", "Noto Sans CJK TC"],
        "zh-TW" => &["Noto Sans CJK TC", "Noto Sans CJK SC"],
        _ => &["Noto Sans CJK SC", "Noto Sans CJK JP"],
    }
}

impl Fallback for NotoFallback {
    fn common_fallback(&self) -> &'static [&'static str] {
        &[
            "Noto Sans",
            "Noto Sans Mono",
            "Noto Sans Symbols",
            "Noto Sans Symbols2",
            "Noto Color Emoji",
            "Noto Sans CJK SC",
        ]
    }

    fn forbidden_fallback(&self) -> &'static [&'static str] {
        &[]
    }

    fn script_fallback(&self, script: Script, locale: &str) -> &'static [&'static str] {
        match script {
            Script::Arabic => &["Noto Sans Arabic", "Noto Naskh Arabic"],
            Script::Armenian => &["Noto Sans Armenian"],
            Script::Bengali => &["Noto Sans Bengali"],
            Script::Devanagari => &["Noto Sans Devanagari"],
            Script::Ethiopic => &["Noto Sans Ethiopic"],
            Script::Georgian => &["Noto Sans Georgian"],
            Script::Gujarati => &["Noto Sans Gujarati"],
            Script::Gurmukhi => &["Noto Sans Gurmukhi"],
            Script::Hebrew => &["Noto Sans Hebrew"],
            Script::Kannada => &["Noto Sans Kannada"],
            Script::Khmer => &["Noto Sans Khmer"],
            Script::Lao => &["Noto Sans Lao"],
            Script::Malayalam => &["Noto Sans Malayalam"],
            Script::Myanmar => &["Noto Sans Myanmar"],
            Script::Sinhala => &["Noto Sans Sinhala"],
            Script::Tamil => &["Noto Sans Tamil"],
            Script::Telugu => &["Noto Sans Telugu"],
            Script::Thai => &["Noto Sans Thai"],
            Script::Hangul => &["Noto Sans CJK KR"],
            Script::Han | Script::Hiragana | Script::Katakana | Script::Bopomofo => han(locale),
            _ => &[],
        }
    }
}

/// One rasterised glyph, relative to its pen position on the baseline.
struct Raster {
    left: i32,
    top: i32,
    width: usize,
    /// Coverage per pixel (`Mask`) or straight RGBA (`Color`).
    data: Vec<u8>,
    color: bool,
}

struct Placed {
    x: i32,
    y: i32,
    raster: Option<Rc<Raster>>,
}

/// A shaped, visually ordered line laid out at pen 0 on baseline 0.
struct Shaped {
    width: f32,
    glyphs: Vec<Placed>,
    /// At least one glyph is the missing-glyph (id 0) of every tried face.
    missing: bool,
}

type LineKey = (Face, Weight, u32);

struct Engine {
    fonts: FontSystem,
    swash: SwashCache,
    rasters: HashMap<CacheKey, Option<Rc<Raster>>>,
    lines: HashMap<LineKey, HashMap<Box<str>, Rc<Shaped>>>,
    line_count: usize,
    /// Units-per-em, ascent, descent (negative) and leading of each embedded
    /// face, in font units; line metrics always come from Plex.
    metrics: HashMap<(Face, Weight), [f32; 4]>,
    guest_dirs: Vec<String>,
    guest_loaded: bool,
}

/// The text system handed to every draw call. Interior mutability keeps the
/// caches behind `&self`, like the canvas helpers.
pub struct Type {
    engine: RefCell<Engine>,
}

fn locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|k| std::env::var(k).ok())
        .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
        .map(|v| {
            v.split(['.', '@'])
                .next()
                .unwrap_or_default()
                .replace('_', "-")
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "en-US".to_owned())
}

impl Type {
    /// The embedded Plex faces with fallback from the guest font directories
    /// (`/usr/share/fonts`, or `FERRIX_FONT_DIRS`), scanned on first need.
    pub fn load() -> Result<Self, String> {
        let dirs = match std::env::var("FERRIX_FONT_DIRS") {
            Ok(v) => v
                .split(':')
                .filter(|d| !d.is_empty())
                .map(str::to_owned)
                .collect(),
            Err(_) => DEFAULT_FONT_DIRS.iter().map(|d| (*d).to_owned()).collect(),
        };
        Self::with_font_dirs(dirs)
    }

    /// Only the embedded faces: no fallback directory is ever scanned. For
    /// deterministic tests of the Plex path.
    pub fn embedded() -> Result<Self, String> {
        Self::with_font_dirs(Vec::new())
    }

    /// The embedded faces plus fallback from exactly `dirs`.
    pub fn with_font_dirs(dirs: Vec<String>) -> Result<Self, String> {
        let mut db = fontdb::Database::new();
        let mut ids = Vec::new();
        for (face, weight, bytes) in &EMBEDDED {
            let source = fontdb::Source::Binary(Arc::new(*bytes));
            let loaded = db.load_font_source(source);
            let id = *loaded
                .first()
                .ok_or_else(|| format!("embedded {face:?} {weight:?} face did not parse"))?;
            ids.push((*face, *weight, id));
        }
        db.set_sans_serif_family(SANS_FAMILY);
        db.set_monospace_family(MONO_FAMILY);
        let mut fonts = FontSystem::new_with_locale_and_db_and_fallback(locale(), db, NotoFallback);
        let mut metrics = HashMap::new();
        for (face, weight, id) in ids {
            let font = fonts
                .get_font(id, fontdb::Weight(primary_weight(face, weight)))
                .ok_or_else(|| format!("embedded {face:?} {weight:?} face is unusable"))?;
            let m = font.metrics();
            metrics.insert(
                (face, weight),
                [f32::from(m.units_per_em), m.ascent, m.descent, m.leading],
            );
        }
        Ok(Self {
            engine: RefCell::new(Engine {
                fonts,
                swash: SwashCache::new(),
                rasters: HashMap::new(),
                lines: HashMap::new(),
                line_count: 0,
                metrics,
                guest_dirs: dirs,
                guest_loaded: false,
            }),
        })
    }

    fn shaped(&self, style: Style, text: &str) -> Rc<Shaped> {
        let key = (style.face, style.weight, style.px.to_bits());
        let mut engine = self.engine.borrow_mut();
        if let Some(line) = engine.lines.get(&key).and_then(|m| m.get(text)) {
            return Rc::clone(line);
        }
        let mut line = engine.shape(style, text);
        if line.missing && !engine.guest_loaded {
            engine.load_guest_fonts();
            line = engine.shape(style, text);
        }
        let line = Rc::new(line);
        if engine.line_count >= LINE_CACHE_LIMIT {
            engine.lines.clear();
            engine.line_count = 0;
        }
        engine.line_count += 1;
        engine
            .lines
            .entry(key)
            .or_default()
            .insert(text.into(), Rc::clone(&line));
        line
    }

    /// Whether every character of `text` has a real glyph in some loaded
    /// face (embedded or guest fallback). Lets callers substitute before
    /// drawing tofu.
    pub fn has_glyphs(&self, weight: Weight, text: &str) -> bool {
        let style = Style {
            weight,
            ..Style::regular(16.0, 0)
        };
        !text.is_empty() && !self.shaped(style, text).missing
    }

    /// Shaped advance of `text`, rounded to whole pixels.
    pub fn width(&self, style: Style, text: &str) -> i32 {
        if text.is_empty() {
            return 0;
        }
        self.shaped(style, text).width.round() as i32
    }

    /// Shaped advance of one character; for [`Face::Mono`] and an ASCII
    /// character this is the terminal cell width.
    pub fn advance(&self, style: Style, ch: char) -> f32 {
        let mut buf = [0u8; 4];
        self.shaped(style, ch.encode_utf8(&mut buf)).width
    }

    fn line_metrics(&self, style: Style) -> [f32; 4] {
        self.engine.borrow().metrics[&(style.face, style.weight)]
    }

    /// Distance from the top of a line box to its baseline (from Plex).
    pub fn ascent(&self, style: Style) -> i32 {
        let [upem, ascent, _, _] = self.line_metrics(style);
        (ascent * style.px / upem).round() as i32
    }

    /// Recommended line box height for the face at this size (from Plex).
    pub fn line_height(&self, style: Style) -> i32 {
        let [upem, ascent, descent, leading] = self.line_metrics(style);
        ((ascent - descent + leading) * style.px / upem).round() as i32
    }

    /// Draws one character with its pen position at `x`. Returns the
    /// advance so callers can lay out a fixed grid.
    pub fn char(&self, canvas: &mut Canvas, style: Style, ch: char, x: i32, baseline: i32) -> f32 {
        let mut buf = [0u8; 4];
        let line = self.shaped(style, ch.encode_utf8(&mut buf));
        draw_line(canvas, style.color, &line, x, baseline);
        line.width
    }

    /// Draws one terminal cell: the cluster `text` (a base character plus
    /// any combining marks the emulator attached to it) centred in a span of
    /// `span_w` pixels starting at `x` (one column's width, or two for a
    /// double-width cell). Plex Mono glyphs fill exactly one column;
    /// double-width CJK and emoji span two, and a fallback glyph of any
    /// advance stays inside its span instead of drifting the grid. Shaping
    /// stays inside the cell: a grid never joins or reorders across columns.
    pub fn cell(
        &self,
        canvas: &mut Canvas,
        style: Style,
        text: &str,
        x: i32,
        baseline: i32,
        span_w: i32,
    ) {
        if text.is_empty() {
            return;
        }
        let line = self.shaped(style, text);
        let pen = x + ((span_w as f32 - line.width) / 2.0).round().max(0.0) as i32;
        draw_line(canvas, style.color, &line, pen, baseline);
    }

    /// Draws a shaped, bidi-ordered line with its left edge at `x`.
    pub fn text(&self, canvas: &mut Canvas, style: Style, text: &str, x: i32, baseline: i32) {
        if text.is_empty() {
            return;
        }
        let line = self.shaped(style, text);
        draw_line(canvas, style.color, &line, x, baseline);
    }
}

fn primary_weight(face: Face, weight: Weight) -> u16 {
    match (face, weight) {
        (_, Weight::Regular) => 400,
        (Face::Sans, Weight::Bold) => 600,
        (Face::Mono, Weight::Bold) => 700,
    }
}

impl Engine {
    fn load_guest_fonts(&mut self) {
        self.guest_loaded = true;
        let db = self.fonts.db_mut();
        for dir in &self.guest_dirs {
            db.load_fonts_dir(dir);
        }
        // Earlier shapes may have used tofu where a guest face now applies.
        self.lines.clear();
        self.line_count = 0;
    }

    fn shape(&mut self, style: Style, text: &str) -> Shaped {
        let [upem, ascent, descent, leading] = self.metrics[&(style.face, style.weight)];
        let line_height = ((ascent - descent + leading) * style.px / upem).max(1.0);
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.px, line_height));
        buffer.set_wrap(Wrap::None);
        buffer.set_size(None, None);
        let family = match style.face {
            Face::Sans => SANS_FAMILY,
            Face::Mono => MONO_FAMILY,
        };
        let attrs = Attrs::new()
            .family(Family::Name(family))
            .weight(fontdb::Weight(primary_weight(style.face, style.weight)));
        // One visual line: callers lay out lines themselves.
        let single: String = text
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        buffer.set_text(&single, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        let mut width = 0.0f32;
        let mut missing = false;
        let mut keys = Vec::new();
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            for glyph in run.glyphs {
                missing |= glyph.glyph_id == 0;
                let physical = glyph.physical((0.0, 0.0), 1.0);
                keys.push((physical.cache_key, physical.x, physical.y));
            }
        }
        drop(buffer);
        let glyphs = keys
            .into_iter()
            .map(|(key, x, y)| Placed {
                x,
                y,
                raster: self.raster(key),
            })
            .collect();
        Shaped {
            width,
            glyphs,
            missing,
        }
    }

    fn raster(&mut self, key: CacheKey) -> Option<Rc<Raster>> {
        if let Some(r) = self.rasters.get(&key) {
            return r.clone();
        }
        let raster = self
            .swash
            .get_image_uncached(&mut self.fonts, key)
            .and_then(|image| {
                let color = match image.content {
                    SwashContent::Mask => false,
                    SwashContent::Color => true,
                    // Subpixel masks are never requested; treat as coverage.
                    SwashContent::SubpixelMask => false,
                };
                let width = image.placement.width as usize;
                (width > 0 && image.placement.height > 0).then(|| {
                    Rc::new(Raster {
                        left: image.placement.left,
                        top: image.placement.top,
                        width,
                        data: if image.content == SwashContent::SubpixelMask {
                            image.data.chunks(4).map(|px| px[1]).collect()
                        } else {
                            image.data
                        },
                        color,
                    })
                })
            });
        self.rasters.insert(key, raster.clone());
        raster
    }
}

fn draw_line(canvas: &mut Canvas, color: u32, line: &Shaped, x: i32, baseline: i32) {
    for glyph in &line.glyphs {
        let Some(r) = &glyph.raster else {
            continue;
        };
        let gx = x + glyph.x + r.left;
        let gy = baseline + glyph.y - r.top;
        if r.color {
            for (row, chunk) in r.data.chunks(r.width * 4).enumerate() {
                for (col, px) in chunk.as_chunks::<4>().0.iter().enumerate() {
                    let rgb = u32::from(px[0]) << 16 | u32::from(px[1]) << 8 | u32::from(px[2]);
                    canvas.blend(gx + col as i32, gy + row as i32, rgb, px[3]);
                }
            }
        } else {
            for (row, chunk) in r.data.chunks(r.width).enumerate() {
                for (col, alpha) in chunk.iter().enumerate() {
                    canvas.blend(gx + col as i32, gy + row as i32, color, *alpha);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Leftmost and rightmost inked columns in a canvas region.
    fn ink_columns(c: &Canvas, y0: i32, y1: i32, bg: u32) -> Vec<i32> {
        (0..c.width())
            .filter(|&x| (y0..y1).any(|y| c.get(x, y) != bg))
            .collect()
    }

    #[test]
    fn plex_faces_load_and_report_line_metrics() {
        let ty = Type::embedded().unwrap();
        let sans = Style::regular(40.0, 0xffffff);
        // Plex: hhea ascender 1025, descender -275, no line gap, 1000 upem.
        assert_eq!(ty.ascent(sans), 41);
        assert_eq!(ty.line_height(sans), 52);
        // Plex Mono is a true monospace: every ASCII glyph is 600 units.
        let mono = Style::mono(40.0, 0xffffff);
        for ch in ['M', 'i', '.', 'W'] {
            assert!((ty.advance(mono, ch) - 24.0).abs() < 0.01, "{ch}");
        }
        // Plex Sans Condensed is proportional and narrower than the mono.
        assert!(ty.width(sans, "iiii") < ty.width(sans, "MMMM"));
        assert!(ty.width(sans, "Settings") < ty.width(mono, "Settings"));
        // SemiBold is wider than Regular.
        assert!(ty.width(Style::bold(40.0, 0), "Settings") > ty.width(sans, "Settings"));
    }

    #[test]
    fn latin_draws_ink_and_unsupported_scripts_report_missing_without_fallback() {
        let ty = Type::embedded().unwrap();
        assert!(ty.has_glyphs(Weight::Regular, "Omarchy Ferrix \u{2026}"));
        assert!(ty.has_glyphs(Weight::Bold, "Wi-Fi 5 GHz"));
        assert!(!ty.has_glyphs(Weight::Regular, ""));
        // No guest fonts here: Arabic, Devanagari and Han are honestly missing.
        for s in [
            "\u{0633}\u{0644}\u{0627}\u{0645}",
            "\u{0928}\u{092e}",
            "\u{4e2d}",
        ] {
            assert!(!ty.has_glyphs(Weight::Regular, s), "{s}");
        }
        let mut c = Canvas::new(200, 60, 0x000000);
        ty.text(&mut c, Style::regular(32.0, 0xffffff), "Ferrix", 10, 40);
        let ink = ink_columns(&c, 0, 60, 0x000000);
        assert!(!ink.is_empty());
        assert!(
            ink[0] >= 10
                && *ink.last().unwrap() < 10 + ty.width(Style::regular(32.0, 0), "Ferrix") + 2
        );
    }

    #[test]
    fn terminal_cells_centre_their_glyph_in_the_span() {
        let ty = Type::embedded().unwrap();
        let style = Style::mono(40.0, 0xffffff);
        let cell_w = ty.advance(style, 'M').round() as i32;
        let mut one = Canvas::new(200, 60, 0x000000);
        ty.cell(&mut one, style, "I", 0, 45, cell_w);
        let mut two = Canvas::new(200, 60, 0x000000);
        ty.cell(&mut two, style, "I", 0, 45, 2 * cell_w);
        let a = ink_columns(&one, 0, 60, 0);
        let b = ink_columns(&two, 0, 60, 0);
        assert!(*a.last().unwrap() < cell_w);
        // A two-column span moves the glyph by half a cell.
        let shift = b[0] - a[0];
        assert!((shift - cell_w / 2).abs() <= 1, "{shift} vs {cell_w}");
    }

    #[test]
    fn the_licence_ships_with_the_faces() {
        assert!(IBM_PLEX_LICENSE.contains("SIL Open Font License, Version 1.1"));
        assert!(IBM_PLEX_LICENSE.contains("Reserved Font Name \"Plex\""));
        assert!(ATTRIBUTION.contains("OFL"));
    }

    #[test]
    fn newlines_are_flattened_to_one_visual_line() {
        let ty = Type::embedded().unwrap();
        let s = Style::regular(20.0, 0);
        assert_eq!(ty.width(s, "a\nb"), ty.width(s, "a b"));
    }

    #[test]
    fn fallback_order_is_script_specific_and_host_independent() {
        let f = NotoFallback;
        assert_eq!(
            f.script_fallback(Script::Arabic, "en-US")[0],
            "Noto Sans Arabic"
        );
        assert_eq!(
            f.script_fallback(Script::Devanagari, "en-US"),
            &["Noto Sans Devanagari"]
        );
        assert_eq!(
            f.script_fallback(Script::Han, "en-US")[0],
            "Noto Sans CJK SC"
        );
        assert_eq!(
            f.script_fallback(Script::Han, "ja-JP")[0],
            "Noto Sans CJK JP"
        );
        assert_eq!(f.common_fallback()[0], "Noto Sans");
    }
}
