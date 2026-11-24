//! Launcher icons of installed APKs (ADR-008, "Installed APKs").
//!
//! Waydroid's Android side writes one PNG per launchable package into
//! Android's `/data/icons/`, which is the session user's
//! `$XDG_DATA_HOME/waydroid/data/icons/<package>.png` on the host
//! (Waydroid 1.6.3: `tools/config/__init__.py` sets `waydroid_data` to
//! `<xdg_data_home>/waydroid/data`, `tools/helpers/lxc.py` binds it as the
//! container's `/data`, and `tools/services/user_manager.py` points each
//! desktop entry's `Icon=` at `<waydroid_data>/icons/<package>.png`).
//!
//! Those files are written by the container and are untrusted input:
//!
//! - the file name comes from a validated package name only (no `/`, no
//!   `..`), and the file must be a regular file, not a symlink, both before
//!   and after opening (same device and inode);
//! - at most [`MAX_ICON_FILE_BYTES`] are read;
//! - the maintained `png` crate decodes it with a memory limit, and images
//!   wider or taller than [`MAX_DECODED_SIDE`] are refused before their
//!   pixels are allocated;
//! - the result is box-filtered down to [`APP_ICON_MAX_SIDE`] and handed out
//!   as a bounded [`AppIconBitmap`].
//!
//! Any failure is `None`: the home draws the letter glyph.

use std::fs::File;
use std::io::{Cursor, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use ferrix_app_api::{APP_ICON_MAX_SIDE, AppIconBitmap};

use crate::config::validate_package;

/// Largest icon file read (bytes). Waydroid's icons are a few kilobytes.
pub const MAX_ICON_FILE_BYTES: u64 = 1024 * 1024;
/// Largest decoded width or height accepted (pixels).
pub const MAX_DECODED_SIDE: u32 = 1024;
/// Memory limit handed to the PNG decoder (bytes).
const DECODER_MEMORY_LIMIT: usize = 8 * 1024 * 1024;

/// Environment override for the icon directory (an appd account other than
/// the Waydroid session user, or a non-default data home).
pub const WAYDROID_ICON_DIR_ENV: &str = "FERRIX_WAYDROID_ICON_DIR";

/// Where Waydroid's icons are for the account running this process, given
/// an environment lookup: `FERRIX_WAYDROID_ICON_DIR`, else
/// `$XDG_DATA_HOME/waydroid/data/icons`, else
/// `$HOME/.local/share/waydroid/data/icons` (Waydroid's own resolution of
/// its user data directory). Relative values are ignored, as Waydroid's XDG
/// lookup would be wrong for them too. `None` when nothing resolves.
pub fn default_icon_dir(env: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let absolute = |name: &str| {
        env(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
    };
    if let Some(dir) = absolute(WAYDROID_ICON_DIR_ENV) {
        return Some(dir);
    }
    let data_home = absolute("XDG_DATA_HOME")
        .or_else(|| absolute("HOME").map(|home| home.join(".local/share")))?;
    Some(data_home.join("waydroid/data/icons"))
}

/// Reads `<dir>/<package>.png` with the bounds above. `None` for an invalid
/// package, a missing, oversized, symlinked, or non-regular file.
pub fn read_icon_file(dir: &Path, package: &str) -> Option<Vec<u8>> {
    validate_package(package).ok()?;
    let path = dir.join(format!("{package}.png"));
    let before = std::fs::symlink_metadata(&path).ok()?;
    if !before.file_type().is_file() || before.len() > MAX_ICON_FILE_BYTES {
        return None;
    }
    let file = File::open(&path).ok()?;
    let after = file.metadata().ok()?;
    // The name was swapped between the check and the open.
    if !after.file_type().is_file() || after.dev() != before.dev() || after.ino() != before.ino() {
        return None;
    }
    let mut bytes = Vec::with_capacity(after.len().min(MAX_ICON_FILE_BYTES) as usize);
    file.take(MAX_ICON_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= MAX_ICON_FILE_BYTES).then_some(bytes)
}

/// Decodes an untrusted PNG into a bitmap no larger than
/// [`APP_ICON_MAX_SIDE`] per side (aspect kept, never upscaled).
pub fn decode_icon(bytes: &[u8]) -> Result<AppIconBitmap, String> {
    if bytes.len() as u64 > MAX_ICON_FILE_BYTES {
        return Err("icon file too large".to_owned());
    }
    let mut decoder = png::Decoder::new_with_limits(
        Cursor::new(bytes),
        png::Limits {
            bytes: DECODER_MEMORY_LIMIT,
        },
    );
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| format!("png: {e}"))?;
    let (width, height) = {
        let info = reader.info();
        (info.width, info.height)
    };
    if width == 0 || height == 0 || width > MAX_DECODED_SIDE || height > MAX_DECODED_SIDE {
        return Err(format!("icon {width}x{height} exceeds {MAX_DECODED_SIDE}"));
    }
    let mut buffer = vec![0; reader.output_buffer_size()];
    let frame = reader
        .next_frame(&mut buffer)
        .map_err(|e| format!("png: {e}"))?;
    if frame.bit_depth != png::BitDepth::Eight {
        return Err(format!("unexpected bit depth {:?}", frame.bit_depth));
    }
    let pixels = (width * height) as usize;
    let data = &buffer[..frame.buffer_size()];
    let rgba: Vec<u8> = match frame.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|&[r, g, b]| [r, g, b, 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|&[g, a]| [g, g, g, a])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => return Err("palette was not expanded".to_owned()),
    };
    if rgba.len() != pixels * 4 {
        return Err("decoded size mismatch".to_owned());
    }
    let icon = downscale(width, height, &rgba);
    if icon.is_blank() {
        return Err("icon is fully transparent".to_owned());
    }
    Ok(icon)
}

/// The decoded icon of `package` under `dir`, or `None` on any failure.
pub fn load_icon(dir: &Path, package: &str) -> Option<AppIconBitmap> {
    decode_icon(&read_icon_file(dir, package)?).ok()
}

/// Box filter (area average in premultiplied alpha) to fit
/// [`APP_ICON_MAX_SIDE`]; images that already fit are kept as they are.
fn downscale(width: u32, height: u32, rgba: &[u8]) -> AppIconBitmap {
    let max = APP_ICON_MAX_SIDE;
    let (dw, dh) = if width <= max && height <= max {
        (width, height)
    } else if width >= height {
        (max, (height * max / width).max(1))
    } else {
        ((width * max / height).max(1), max)
    };
    if (dw, dh) == (width, height) {
        return AppIconBitmap::new(width, height, rgba.to_vec()).expect("bounded source");
    }
    let mut out = Vec::with_capacity((dw * dh * 4) as usize);
    for dy in 0..dh {
        let (y0, y1) = span(dy, dh, height);
        for dx in 0..dw {
            let (x0, x1) = span(dx, dw, width);
            let mut sum = [0u64; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = ((y * width + x) * 4) as usize;
                    let a = u64::from(rgba[i + 3]);
                    sum[0] += u64::from(rgba[i]) * a;
                    sum[1] += u64::from(rgba[i + 1]) * a;
                    sum[2] += u64::from(rgba[i + 2]) * a;
                    sum[3] += a;
                }
            }
            let count = u64::from((x1 - x0) * (y1 - y0));
            if sum[3] == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                // Un-premultiply: colour = sum(c*a)/sum(a); alpha = mean.
                let c = |s: u64| ((s + sum[3] / 2) / sum[3]).min(255) as u8;
                out.extend_from_slice(&[
                    c(sum[0]),
                    c(sum[1]),
                    c(sum[2]),
                    ((sum[3] + count / 2) / count) as u8,
                ]);
            }
        }
    }
    AppIconBitmap::new(dw, dh, out).expect("downscaled within bounds")
}

/// Source range `[start, end)` covered by destination index `d` of `dn`.
fn span(d: u32, dn: u32, sn: u32) -> (u32, u32) {
    let start = d * sn / dn;
    let end = ((d + 1) * sn / dn).max(start + 1).min(sn);
    (start, end)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Encodes a test PNG (mock input).
    pub(crate) fn png_bytes(
        width: u32,
        height: u32,
        color: png::ColorType,
        data: &[u8],
    ) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut out, width, height);
            encoder.set_color(color);
            encoder.set_depth(png::BitDepth::Eight);
            let mut writer = encoder.write_header().unwrap();
            writer.write_image_data(data).unwrap();
        }
        out
    }

    pub(crate) fn solid_rgba(width: u32, height: u32, px: [u8; 4]) -> Vec<u8> {
        png_bytes(
            width,
            height,
            png::ColorType::Rgba,
            &px.repeat((width * height) as usize),
        )
    }

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrix-icon-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn small_icons_decode_unchanged_and_large_ones_are_downscaled() {
        let small = decode_icon(&solid_rgba(48, 48, [10, 20, 30, 255])).unwrap();
        assert_eq!((small.width(), small.height()), (48, 48));
        assert_eq!(&small.rgba()[..4], &[10, 20, 30, 255]);

        let big = decode_icon(&solid_rgba(192, 192, [200, 100, 50, 255])).unwrap();
        assert_eq!((big.width(), big.height()), (96, 96));
        assert!(
            big.rgba()
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [200, 100, 50, 255])
        );

        let wide = decode_icon(&solid_rgba(400, 100, [1, 2, 3, 255])).unwrap();
        assert_eq!((wide.width(), wide.height()), (96, 24));
    }

    #[test]
    fn downscaling_averages_in_premultiplied_alpha() {
        // 2x1 -> must not darken: an opaque red beside a transparent black.
        let rgba = [255, 0, 0, 255, 0, 0, 0, 0].repeat(96);
        let icon = downscale(192, 1, &rgba);
        assert_eq!((icon.width(), icon.height()), (96, 1));
        assert_eq!(&icon.rgba()[..4], &[255, 0, 0, 128]);
    }

    #[test]
    fn other_colour_types_become_rgba() {
        let gray = decode_icon(&png_bytes(1, 1, png::ColorType::Grayscale, &[77])).unwrap();
        assert_eq!(gray.rgba(), &[77, 77, 77, 255]);
        let rgb = decode_icon(&png_bytes(1, 1, png::ColorType::Rgb, &[1, 2, 3])).unwrap();
        assert_eq!(rgb.rgba(), &[1, 2, 3, 255]);
        let ga = decode_icon(&png_bytes(1, 1, png::ColorType::GrayscaleAlpha, &[9, 99])).unwrap();
        assert_eq!(ga.rgba(), &[9, 9, 9, 99]);
    }

    #[test]
    fn hostile_or_broken_files_are_refused() {
        assert!(decode_icon(b"").is_err());
        assert!(decode_icon(b"not a png at all").is_err());
        let mut truncated = solid_rgba(64, 64, [1, 1, 1, 255]);
        truncated.truncate(truncated.len() / 2);
        assert!(decode_icon(&truncated).is_err());
        // Oversized dimensions are refused (the header alone says so).
        let tall = png_bytes(1, 2000, png::ColorType::Grayscale, &[0; 2000]);
        assert!(decode_icon(&tall).unwrap_err().contains("exceeds"));
        // Fully transparent icons are useless; the letter glyph is better.
        assert!(decode_icon(&solid_rgba(8, 8, [0, 0, 0, 0])).is_err());
        // An oversized byte stream is refused before decoding.
        assert!(decode_icon(&vec![0; MAX_ICON_FILE_BYTES as usize + 1]).is_err());
    }

    #[test]
    fn icon_files_are_read_by_package_name_only_and_never_through_symlinks() {
        let dir = temp_dir("read");
        let good = solid_rgba(4, 4, [5, 6, 7, 255]);
        std::fs::write(dir.join("org.example.notes.png"), &good).unwrap();
        assert_eq!(
            read_icon_file(&dir, "org.example.notes"),
            Some(good.clone())
        );
        assert!(load_icon(&dir, "org.example.notes").is_some());
        // Missing file, invalid package names (no traversal).
        assert_eq!(read_icon_file(&dir, "org.example.absent"), None);
        assert_eq!(read_icon_file(&dir, "../etc/passwd"), None);
        assert_eq!(read_icon_file(&dir, "notes"), None);
        // Symlinks are refused even to a valid icon.
        std::os::unix::fs::symlink(
            dir.join("org.example.notes.png"),
            dir.join("org.example.link.png"),
        )
        .unwrap();
        assert_eq!(read_icon_file(&dir, "org.example.link"), None);
        // Directories and oversized files are refused.
        std::fs::create_dir(dir.join("org.example.dir.png")).unwrap();
        assert_eq!(read_icon_file(&dir, "org.example.dir"), None);
        let big = File::create(dir.join("org.example.big.png")).unwrap();
        big.set_len(MAX_ICON_FILE_BYTES + 1).unwrap();
        assert_eq!(read_icon_file(&dir, "org.example.big"), None);
        // A corrupt file yields no icon.
        std::fs::write(dir.join("org.example.bad.png"), b"\x89PNG garbage").unwrap();
        assert_eq!(load_icon(&dir, "org.example.bad"), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn icon_directory_follows_waydroid_data_home_resolution() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| (*v).to_owned())
            }
        };
        assert_eq!(
            default_icon_dir(env(&[("HOME", "/home/user")])),
            Some(PathBuf::from("/home/user/.local/share/waydroid/data/icons"))
        );
        assert_eq!(
            default_icon_dir(env(&[
                ("HOME", "/home/user"),
                ("XDG_DATA_HOME", "/data/xdg")
            ])),
            Some(PathBuf::from("/data/xdg/waydroid/data/icons"))
        );
        assert_eq!(
            default_icon_dir(env(&[
                ("HOME", "/home/user"),
                (WAYDROID_ICON_DIR_ENV, "/srv/icons"),
            ])),
            Some(PathBuf::from("/srv/icons"))
        );
        assert_eq!(
            default_icon_dir(env(&[("HOME", "relative"), ("XDG_DATA_HOME", "")])),
            None
        );
        assert_eq!(default_icon_dir(env(&[])), None);
    }
}
