//! The Ferrix UI glyph set: a pinned subset of [Remix Icon] v4.7.0
//! (Apache-2.0), compiled into outline data by `build.rs` and filled through
//! the `tiny-skia` software rasteriser at the exact pixel size each surface
//! asks for.
//!
//! Rules (`docs/design.md`, "Icons"):
//!
//! - Glyphs are UI symbols only, never a logo or brand mark.
//! - The filled variant is used wherever Remix has one, in every state.
//!   Inactive versus active is shown with colour (`fg_dim` versus `fg` or the
//!   accent), never by swapping a line glyph for a filled one.
//! - An icon is additive: the text label or accessible name it accompanies
//!   stays.
//!
//! There is no run-time SVG parsing, font, file, or network access; the
//! outlines are `static` data in the binary.
//!
//! [Remix Icon]: https://github.com/Remix-Design/RemixIcon/tree/v4.7.0

use crate::draw::{Canvas, Rect};

/// Upstream licence text of the vendored subset, for surfaces that display
/// third-party notices. Images ship the same file as
/// `/usr/share/licenses/ferrix/remixicon-4.7.0/License` (Apache-2.0 section
/// 4(a)).
pub const REMIX_LICENSE: &str = include_str!("../assets/icons/remixicon/License");

/// One-line attribution for About pages and notices.
pub const ATTRIBUTION: &str = "Icons: Remix Icon 4.7.0, Apache-2.0";

/// One compiled outline in the 24x24 Remix grid.
pub(crate) struct Outline {
    pub name: &'static str,
    pub even_odd: bool,
    pub ops: &'static [u8],
    pub pts: &'static [f32],
}

include!(concat!(env!("OUT_DIR"), "/icons.rs"));

/// Every glyph the system uses. Names say what the glyph means in Ferrix; the
/// Remix source name is [`Icon::source`].
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
    Battery,
    BatteryLow,
    BatteryCharging,
    Bluetooth,
    Notifications,
    Brightness,
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

impl Icon {
    pub const ALL: [Icon; 36] = [
        Icon::Phone,
        Icon::Messages,
        Icon::Contacts,
        Icon::Settings,
        Icon::Terminal,
        Icon::Camera,
        Icon::Browser,
        Icon::WifiOff,
        Icon::WifiLow,
        Icon::WifiMid,
        Icon::WifiFull,
        Icon::Battery,
        Icon::BatteryLow,
        Icon::BatteryCharging,
        Icon::Bluetooth,
        Icon::Notifications,
        Icon::Brightness,
        Icon::Home,
        Icon::Back,
        Icon::Forward,
        Icon::More,
        Icon::Add,
        Icon::Close,
        Icon::Backspace,
        Icon::Dialpad,
        Icon::History,
        Icon::Folder,
        Icon::File,
        Icon::FileText,
        Icon::FileImage,
        Icon::FileAudio,
        Icon::FileVideo,
        Icon::FileArchive,
        Icon::FileCode,
        Icon::Link,
        Icon::FileUnknown,
    ];

    /// The Remix Icon v4.7.0 file stem. Every entry is a `-fill` variant.
    pub fn source(self) -> &'static str {
        match self {
            Icon::Phone => "phone-fill",
            Icon::Messages => "message-2-fill",
            Icon::Contacts => "contacts-book-fill",
            Icon::Settings => "settings-3-fill",
            Icon::Terminal => "terminal-box-fill",
            Icon::Camera => "camera-fill",
            Icon::Browser => "global-fill",
            Icon::WifiOff => "signal-wifi-off-fill",
            Icon::WifiLow => "signal-wifi-1-fill",
            Icon::WifiMid => "signal-wifi-2-fill",
            Icon::WifiFull => "signal-wifi-fill",
            Icon::Battery => "battery-fill",
            Icon::BatteryLow => "battery-low-fill",
            Icon::BatteryCharging => "battery-charge-fill",
            Icon::Bluetooth => "bluetooth-fill",
            Icon::Notifications => "notification-3-fill",
            Icon::Brightness => "sun-fill",
            Icon::Home => "home-5-fill",
            Icon::Back => "arrow-left-s-fill",
            Icon::Forward => "arrow-right-s-fill",
            Icon::More => "more-fill",
            Icon::Add => "add-fill",
            Icon::Close => "close-fill",
            Icon::Backspace => "delete-back-2-fill",
            Icon::Dialpad => "grid-fill",
            Icon::History => "history-fill",
            Icon::Folder => "folder-fill",
            Icon::File => "file-fill",
            Icon::FileText => "file-text-fill",
            Icon::FileImage => "file-image-fill",
            Icon::FileAudio => "file-music-fill",
            Icon::FileVideo => "file-video-fill",
            Icon::FileArchive => "file-zip-fill",
            Icon::FileCode => "file-code-fill",
            Icon::Link => "links-fill",
            Icon::FileUnknown => "file-unknow-fill",
        }
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

    /// The battery glyph: charging shows the bolt, below 20 % the low glyph,
    /// otherwise the full one. Remix's half-level `battery-2` is a portrait
    /// battery that does not match the others, so it is not used; the exact
    /// percentage is always printed beside the glyph.
    pub fn battery(percent: u8, charging: bool) -> Icon {
        match (charging, percent) {
            (true, _) => Icon::BatteryCharging,
            (false, p) if p < 20 => Icon::BatteryLow,
            _ => Icon::Battery,
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

/// Fills `icon` into the square `size` px box whose top-left is (`x`, `y`),
/// tinted `color`. The 24-unit grid scales exactly to `size`, so callers keep
/// `size` a whole number of pixels for crisp edges.
pub fn draw(c: &mut Canvas, icon: Icon, x: i32, y: i32, size: i32, color: u32) {
    if size <= 0 {
        return;
    }
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
    let Some(path) = pb.finish() else {
        return;
    };
    let k = size as f32 / 24.0;
    let transform = tiny_skia::Transform::from_row(k, 0.0, 0.0, k, x as f32, y as f32);
    c.fill_path(&path, color, outline.even_odd, transform);
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
    fn every_icon_has_a_compiled_filled_outline_and_none_is_unused() {
        for icon in Icon::ALL {
            assert!(icon.source().ends_with("-fill"), "{icon:?} must be filled");
            assert!(OUTLINES.iter().any(|o| o.name == icon.source()), "{icon:?}");
        }
        for outline in OUTLINES {
            assert!(
                Icon::ALL.iter().any(|i| i.source() == outline.name),
                "{} is vendored but no Icon uses it",
                outline.name
            );
        }
        assert_eq!(OUTLINES.len(), Icon::ALL.len());
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
                        (7..57).contains(&x) && (7..57).contains(&y),
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
    fn level_and_file_mappings() {
        assert_eq!(Icon::wifi(0), Icon::WifiOff);
        assert_eq!(Icon::wifi(3), Icon::WifiFull);
        assert_eq!(Icon::battery(10, true), Icon::BatteryCharging);
        assert_eq!(Icon::battery(10, false), Icon::BatteryLow);
        assert_eq!(Icon::battery(90, false), Icon::Battery);
        assert_eq!(Icon::for_file_name("notes.TXT"), Icon::FileText);
        assert_eq!(Icon::for_file_name("main.rs"), Icon::FileCode);
        assert_eq!(Icon::for_file_name(".bashrc"), Icon::File);
        assert_eq!(Icon::for_file_name("README"), Icon::File);
        assert_eq!(Icon::for_app("files"), Some(Icon::Folder));
        assert_eq!(Icon::for_app("unknown"), None);
    }

    #[test]
    fn the_licence_ships_with_the_glyphs() {
        assert!(REMIX_LICENSE.contains("Apache License"));
        assert!(REMIX_LICENSE.contains("Version 2.0, January 2004"));
    }
}
