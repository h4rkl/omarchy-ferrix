//! The Ferrix UI glyph set: a pinned subset of Google [Material Symbols]
//! (Apache-2.0), Rounded family, weight 400, grade 0, optical size 24, FILL 1.
//! `build.rs` compiles the vendored SVGs into outline data; [`draw`] fills an
//! outline once per (glyph, pixel size) through the `tiny-skia` software
//! rasteriser, caches the coverage mask, and tints it with the caller's theme
//! colour on every later draw.
//!
//! Rules (`docs/design.md`, "Icons"):
//!
//! - Glyphs are UI symbols only, never a logo or brand mark.
//! - The filled (FILL 1) file is used for every glyph, in every state.
//!   Inactive versus active or pressed is shown with colour (`fg_dim` versus
//!   `fg` or the region's one accent), never by switching fill. Every glyph in
//!   the subset has a FILL 1 file at the pin, so none falls back to outlined.
//! - An icon is additive: the text label or accessible name it accompanies
//!   stays.
//!
//! There is no run-time SVG parsing, font, file, or network access; the
//! outlines are `static` data in the binary.
//!
//! [Material Symbols]: https://github.com/google/material-design-icons

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::draw::{Canvas, Rect};

/// Upstream licence text of the vendored subset, for surfaces that display
/// third-party notices. Images ship the same file as
/// `/usr/share/licenses/ferrix/material-symbols/LICENSE` (Apache-2.0 section
/// 4(a)).
pub const MATERIAL_SYMBOLS_LICENSE: &str = include_str!("../assets/icons/material-symbols/LICENSE");

/// One-line attribution for About pages and notices.
pub const ATTRIBUTION: &str = "Icons: Material Symbols (Google), Apache-2.0";

/// One compiled outline in the 24x24 unit grid.
pub(crate) struct Outline {
    pub name: &'static str,
    pub even_odd: bool,
    pub ops: &'static [u8],
    pub pts: &'static [f32],
}

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// Every glyph the system uses. Names say what the glyph means in Ferrix; the
/// Material Symbols source name is [`Icon::source`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    // Launcher applications.
    Phone,
    Messages,
    Contacts,
    Settings,
    Terminal,
    Camera,
    Browser,
    // Status and quick settings.
    WifiOff,
    WifiLow,
    WifiMid,
    WifiFull,
    Cellular0,
    Cellular1,
    Cellular2,
    Cellular3,
    Cellular4,
    Battery0,
    Battery1,
    Battery2,
    Battery3,
    Battery4,
    Battery5,
    Battery6,
    BatteryFull,
    BatteryCharging,
    Bluetooth,
    Notifications,
    Brightness,
    Keyboard,
    // Navigation and actions.
    Home,
    Back,
    Forward,
    More,
    Add,
    Close,
    Backspace,
    Dialpad,
    History,
    // Files types (the launcher's Files tile uses `Folder`).
    Folder,
    File,
    FileText,
    FileImage,
    FileAudio,
    FileVideo,
    FileArchive,
    FileCode,
    Link,
    FileUnknown,
}

const SOURCES: [(Icon, &str); 48] = [
    (Icon::Phone, "call"),
    (Icon::Messages, "chat"),
    (Icon::Contacts, "contacts"),
    (Icon::Settings, "settings"),
    (Icon::Terminal, "terminal"),
    (Icon::Camera, "photo_camera"),
    (Icon::Browser, "language"),
    (Icon::WifiOff, "signal_wifi_off"),
    (Icon::WifiLow, "network_wifi_1_bar"),
    (Icon::WifiMid, "network_wifi_2_bar"),
    (Icon::WifiFull, "signal_wifi_4_bar"),
    (Icon::Cellular0, "signal_cellular_0_bar"),
    (Icon::Cellular1, "signal_cellular_1_bar"),
    (Icon::Cellular2, "signal_cellular_2_bar"),
    (Icon::Cellular3, "signal_cellular_3_bar"),
    (Icon::Cellular4, "signal_cellular_4_bar"),
    (Icon::Battery0, "battery_android_0"),
    (Icon::Battery1, "battery_android_1"),
    (Icon::Battery2, "battery_android_2"),
    (Icon::Battery3, "battery_android_3"),
    (Icon::Battery4, "battery_android_4"),
    (Icon::Battery5, "battery_android_5"),
    (Icon::Battery6, "battery_android_6"),
    (Icon::BatteryFull, "battery_android_full"),
    (Icon::BatteryCharging, "battery_android_bolt"),
    (Icon::Bluetooth, "bluetooth"),
    (Icon::Notifications, "notifications"),
    (Icon::Brightness, "light_mode"),
    (Icon::Keyboard, "keyboard"),
    (Icon::Home, "home"),
    (Icon::Back, "chevron_left"),
    (Icon::Forward, "chevron_right"),
    (Icon::More, "more_horiz"),
    (Icon::Add, "add"),
    (Icon::Close, "close"),
    (Icon::Backspace, "backspace"),
    (Icon::Dialpad, "dialpad"),
    (Icon::History, "history"),
    (Icon::Folder, "folder"),
    (Icon::File, "draft"),
    (Icon::FileText, "description"),
    (Icon::FileImage, "image"),
    (Icon::FileAudio, "audio_file"),
    (Icon::FileVideo, "video_file"),
    (Icon::FileArchive, "folder_zip"),
    (Icon::FileCode, "code_blocks"),
    (Icon::Link, "link"),
    (Icon::FileUnknown, "unknown_document"),
];

impl Icon {
    pub const ALL: [Icon; 48] = {
        let mut all = [Icon::Phone; 48];
        let mut i = 0;
        while i < SOURCES.len() {
            all[i] = SOURCES[i].0;
            i += 1;
        }
        all
    };

    /// The Material Symbols name: `symbols/web/<name>/materialsymbolsrounded/
    /// <name>_fill1_24px.svg` at the commit in `tools/icons/material-symbols.pins`.
    pub fn source(self) -> &'static str {
        SOURCES
            .iter()
            .find(|(icon, _)| *icon == self)
            .map(|(_, name)| *name)
            .expect("every Icon has a source (checked by tests)")
    }

    /// The Wi-Fi status glyph for `arcs` lit levels (0 = no link).
    pub fn wifi(arcs: u8) -> Icon {
        match arcs {
            0 => Icon::WifiOff,
            1 => Icon::WifiLow,
            2 => Icon::WifiMid,
            _ => Icon::WifiFull,
        }
    }

    /// The battery glyph: charging shows the bolt; otherwise the landscape
    /// battery filled to one of seven steps (`battery_android_0`..`6`), then
    /// `full` from 95 %. The exact percentage is always printed beside the
    /// glyph, and low charge is coloured by the caller.
    pub fn battery(percent: u8, charging: bool) -> Icon {
        const STEPS: [Icon; 7] = [
            Icon::Battery0,
            Icon::Battery1,
            Icon::Battery2,
            Icon::Battery3,
            Icon::Battery4,
            Icon::Battery5,
            Icon::Battery6,
        ];
        match (charging, percent.min(100)) {
            (true, _) => Icon::BatteryCharging,
            (false, 95..) => Icon::BatteryFull,
            (false, p) => STEPS[usize::from(p) * 7 / 95],
        }
    }

    /// The cellular glyph for `bars` (0..=4) lit of four.
    pub fn cellular(bars: u8) -> Icon {
        match bars {
            0 => Icon::Cellular0,
            1 => Icon::Cellular1,
            2 => Icon::Cellular2,
            3 => Icon::Cellular3,
            _ => Icon::Cellular4,
        }
    }

    /// The first-party launcher glyph for an application id's last segment
    /// (`org.ferrix.phone` -> `phone`), or `None` for apps without one.
    pub fn for_app(kind: &str) -> Option<Icon> {
        Some(match kind {
            "phone" => Icon::Phone,
            "messages" => Icon::Messages,
            "contacts" => Icon::Contacts,
            "settings" => Icon::Settings,
            "terminal" => Icon::Terminal,
            "camera" => Icon::Camera,
            "files" => Icon::Folder,
            "browser" => Icon::Browser,
            "home" => Icon::Home,
            _ => return None,
        })
    }

    /// A Files entry glyph from its file name's extension.
    pub fn for_file_name(name: &str) -> Icon {
        let ext = name
            .rsplit_once('.')
            .filter(|(stem, _)| !stem.is_empty())
            .map(|(_, ext)| ext.to_ascii_lowercase());
        match ext.as_deref() {
            Some("txt" | "md" | "log" | "csv" | "conf" | "ini" | "pdf") => Icon::FileText,
            Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" | "ppm") => {
                Icon::FileImage
            }
            Some("mp3" | "ogg" | "opus" | "flac" | "wav" | "m4a") => Icon::FileAudio,
            Some("mp4" | "mkv" | "webm" | "mov" | "avi") => Icon::FileVideo,
            Some("zip" | "tar" | "gz" | "tgz" | "xz" | "zst" | "bz2" | "7z") => Icon::FileArchive,
            Some(
                "rs" | "sh" | "py" | "c" | "h" | "js" | "ts" | "json" | "toml" | "yaml" | "yml"
                | "html" | "css",
            ) => Icon::FileCode,
            _ => Icon::File,
        }
    }

    fn outline(self) -> &'static Outline {
        let name = self.source();
        OUTLINES
            .iter()
            .find(|o| o.name == name)
            .expect("every Icon names a compiled outline (checked by tests)")
    }
}

/// A rasterised coverage mask (0..=255 per pixel, row-major, `size` square).
type Mask = Rc<[u8]>;

/// Upper bound on cached masks; a caller cycling through sizes can never hold
/// more than this many.
const CACHE_LIMIT: usize = 256;

thread_local! {
    /// Masks keyed by glyph and pixel size. A mask is colour-independent, so
    /// a theme, state or accent change costs one tinted blend, not a re-fill;
    /// per (size, colour) the work is the same as a cached tinted bitmap.
    static MASKS: RefCell<HashMap<(Icon, i32), Mask>> = RefCell::new(HashMap::new());
}

/// Fills the outline once into a transparent `size` px square with
/// anti-aliasing and keeps the alpha channel.
fn rasterise(icon: Icon, size: i32) -> Mask {
    let outline = icon.outline();
    let mut pb = tiny_skia::PathBuilder::new();
    let mut p = outline.pts.iter().copied();
    let mut next = || p.next().unwrap_or(0.0);
    for op in outline.ops {
        match op {
            0 => {
                let (x, y) = (next(), next());
                pb.move_to(x, y);
            }
            1 => {
                let (x, y) = (next(), next());
                pb.line_to(x, y);
            }
            2 => {
                let (x1, y1, x, y) = (next(), next(), next(), next());
                pb.quad_to(x1, y1, x, y);
            }
            3 => {
                let (x1, y1, x2, y2, x, y) = (next(), next(), next(), next(), next(), next());
                pb.cubic_to(x1, y1, x2, y2, x, y);
            }
            _ => pb.close(),
        }
    }
    let side = size as u32;
    let mut pm = tiny_skia::Pixmap::new(side, side).expect("positive icon size");
    if let Some(path) = pb.finish() {
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(255, 255, 255, 255);
        paint.anti_alias = true;
        let rule = if outline.even_odd {
            tiny_skia::FillRule::EvenOdd
        } else {
            tiny_skia::FillRule::Winding
        };
        let k = size as f32 / 24.0;
        pm.fill_path(
            &path,
            &paint,
            rule,
            tiny_skia::Transform::from_scale(k, k),
            None,
        );
    }
    pm.data()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| px[3])
        .collect()
}

fn mask(icon: Icon, size: i32) -> Mask {
    MASKS.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(mask) = cache.get(&(icon, size)) {
            return Rc::clone(mask);
        }
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        let mask = rasterise(icon, size);
        cache.insert((icon, size), Rc::clone(&mask));
        mask
    })
}

/// Fills `icon` into the square `size` px box whose top-left is (`x`, `y`),
/// tinted `color` (a theme colour such as `fg`, `fg_dim` or an accent). The
/// 24-unit grid scales exactly to `size`; keep it a whole number of pixels for
/// crisp edges. The first draw at a size rasterises and caches the coverage;
/// later draws, in any colour, only blend.
pub fn draw(c: &mut Canvas, icon: Icon, x: i32, y: i32, size: i32, color: u32) {
    if size <= 0 {
        return;
    }
    let mask = mask(icon, size);
    for (row, line) in mask.chunks_exact(size as usize).enumerate() {
        for (col, alpha) in line.iter().enumerate() {
            c.blend(x + col as i32, y + row as i32, color, *alpha);
        }
    }
}

/// [`draw`] centred on (`cx`, `cy`).
pub fn draw_centered(c: &mut Canvas, icon: Icon, cx: i32, cy: i32, size: i32, color: u32) {
    draw(c, icon, cx - size / 2, cy - size / 2, size, color);
}

/// [`draw`] centred in `r`, `size` px square.
pub fn draw_in(c: &mut Canvas, icon: Icon, r: Rect, size: i32, color: u32) {
    draw(
        c,
        icon,
        r.x + (r.w - size) / 2,
        r.y + (r.h - size) / 2,
        size,
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_one_compiled_outline_and_none_is_unused() {
        for icon in Icon::ALL {
            assert!(OUTLINES.iter().any(|o| o.name == icon.source()), "{icon:?}");
            assert_eq!(
                Icon::ALL
                    .iter()
                    .filter(|i| i.source() == icon.source())
                    .count(),
                1,
                "{icon:?} shares its source"
            );
        }
        for outline in OUTLINES {
            assert!(
                Icon::ALL.iter().any(|i| i.source() == outline.name),
                "{} is vendored but no Icon uses it",
                outline.name
            );
        }
        assert_eq!(OUTLINES.len(), Icon::ALL.len());
        assert_eq!(Icon::ALL.len(), SOURCES.len());
    }

    #[test]
    fn the_vendored_subset_matches_the_pins_file() {
        let pins = include_str!("../../../tools/icons/material-symbols.pins");
        let pinned: Vec<&str> = pins
            .lines()
            .filter_map(|l| l.strip_prefix("ICON="))
            .collect();
        let mut used: Vec<&str> = Icon::ALL.iter().map(|i| i.source()).collect();
        let mut sorted = pinned.clone();
        sorted.sort_unstable();
        used.sort_unstable();
        assert_eq!(sorted, used, "pins file and Icon::ALL disagree");
        assert!(pins.contains("MATERIAL_SYMBOLS_VARIANT=fill1_24px"));
        assert!(pins.contains("MATERIAL_SYMBOLS_FAMILY=materialsymbolsrounded"));
    }

    #[test]
    fn outlines_stay_inside_the_24_unit_grid() {
        for outline in OUTLINES {
            assert!(!outline.ops.is_empty(), "{}", outline.name);
            for v in outline.pts {
                assert!((-0.01..=24.01).contains(v), "{}: {v}", outline.name);
            }
        }
    }

    #[test]
    fn masks_have_the_requested_size_and_real_coverage() {
        for size in [17, 24, 40, 96] {
            for icon in Icon::ALL {
                let m = rasterise(icon, size);
                assert_eq!(m.len(), (size * size) as usize, "{icon:?} at {size}");
                let covered = m.iter().filter(|a| **a > 0).count();
                // Every glyph covers a visible share of its box but never all
                // of it (Material glyphs keep a padding margin).
                assert!(covered * 25 > m.len(), "{icon:?} at {size}: {covered}");
                assert!(covered < m.len(), "{icon:?} at {size} fills its box");
                assert!(m.contains(&255), "{icon:?} at {size}: no solid px");
            }
        }
    }

    #[test]
    fn icons_render_tinted_coverage_inside_their_box() {
        let bg = 0x000000;
        let fg = 0xe7aa5a;
        for icon in Icon::ALL {
            let mut c = Canvas::new(64, 64, bg);
            draw(&mut c, icon, 8, 8, 48, fg);
            let mut inside = 0;
            for y in 0..64 {
                for x in 0..64 {
                    let px = c.get(x, y);
                    if px == bg {
                        continue;
                    }
                    assert!(
                        (8..56).contains(&x) && (8..56).contains(&y),
                        "{icon:?} spilled to ({x},{y})"
                    );
                    inside += 1;
                }
            }
            assert!(inside > 48, "{icon:?} drew {inside} px");
            // Solid interior pixels take exactly the tint.
            assert!(
                (0..64).any(|y| (0..64).any(|x| c.get(x, y) == fg)),
                "{icon:?}"
            );
        }
    }

    #[test]
    fn the_cached_mask_is_reused_in_any_theme_colour() {
        let t = crate::theme::get();
        let mut first = Canvas::new(40, 40, t.bg);
        draw(&mut first, Icon::Settings, 0, 0, 40, t.fg);
        let a = mask(Icon::Settings, 40);
        let b = mask(Icon::Settings, 40);
        assert!(Rc::ptr_eq(&a, &b), "second lookup must hit the cache");
        for color in [t.fg, t.fg_dim, t.steel, t.rust] {
            let mut c = Canvas::new(40, 40, t.bg);
            draw(&mut c, Icon::Settings, 0, 0, 40, color);
            // Same coverage, new colour: every solid pixel is the tint and
            // every empty one is the background.
            for (i, alpha) in a.iter().enumerate() {
                let (x, y) = ((i % 40) as i32, (i / 40) as i32);
                match alpha {
                    255 => assert_eq!(c.get(x, y), color),
                    0 => assert_eq!(c.get(x, y), t.bg),
                    _ => {}
                }
            }
        }
        // A different size is its own mask.
        assert_eq!(mask(Icon::Settings, 20).len(), 400);
    }

    #[test]
    fn level_and_file_mappings() {
        assert_eq!(Icon::wifi(0), Icon::WifiOff);
        assert_eq!(Icon::wifi(1), Icon::WifiLow);
        assert_eq!(Icon::wifi(2), Icon::WifiMid);
        assert_eq!(Icon::wifi(3), Icon::WifiFull);
        assert_eq!(Icon::battery(10, true), Icon::BatteryCharging);
        assert_eq!(Icon::battery(0, false), Icon::Battery0);
        assert_eq!(Icon::battery(10, false), Icon::Battery0);
        assert_eq!(Icon::battery(50, false), Icon::Battery3);
        assert_eq!(Icon::battery(94, false), Icon::Battery6);
        assert_eq!(Icon::battery(95, false), Icon::BatteryFull);
        assert_eq!(Icon::battery(255, false), Icon::BatteryFull);
        assert_eq!(Icon::cellular(0), Icon::Cellular0);
        assert_eq!(Icon::cellular(4), Icon::Cellular4);
        assert_eq!(Icon::cellular(9), Icon::Cellular4);
        assert_eq!(Icon::for_file_name("notes.TXT"), Icon::FileText);
        assert_eq!(Icon::for_file_name("main.rs"), Icon::FileCode);
        assert_eq!(Icon::for_file_name(".bashrc"), Icon::File);
        assert_eq!(Icon::for_file_name("README"), Icon::File);
        assert_eq!(Icon::for_app("files"), Some(Icon::Folder));
        assert_eq!(Icon::for_app("unknown"), None);
    }

    #[test]
    fn the_licence_ships_with_the_glyphs() {
        assert!(MATERIAL_SYMBOLS_LICENSE.contains("Apache License"));
        assert!(MATERIAL_SYMBOLS_LICENSE.contains("Version 2.0, January 2004"));
        assert!(ATTRIBUTION.contains("Apache-2.0"));
    }
}
