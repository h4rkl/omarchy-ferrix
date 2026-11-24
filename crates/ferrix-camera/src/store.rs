//! Saving stills to `~/Pictures`: upright (rotation applied, never
//! mirrored) lossless PNG, named by local time, written to a temporary
//! file and renamed so Files never lists a half-written picture.

// The Wayland half (app.rs) is Linux-only; host builds use the rest in tests
// and previews.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use ferrix_hardware_api::CameraFrame;

use crate::model::{Saved, upright_pixels};

/// The XDG pictures folder below home (Files' `show-pictures` target).
pub const PICTURES: &str = "Pictures";

pub fn pictures_dir(home: &Path) -> PathBuf {
    home.join(PICTURES)
}

/// `IMG_YYYYMMDD_HHMMSS` in the session's zone.
pub fn still_stem(now: &jiff::Zoned) -> String {
    now.strftime("IMG_%Y%m%d_%H%M%S").to_string()
}

/// The first free `<stem>.png`, `<stem>_1.png`, ... in `dir`.
fn free_name(dir: &Path, stem: &str) -> String {
    (0..1000)
        .map(|n| {
            if n == 0 {
                format!("{stem}.png")
            } else {
                format!("{stem}_{n}.png")
            }
        })
        .find(|name| !dir.join(name).exists())
        .unwrap_or_else(|| format!("{stem}_x.png"))
}

/// Encodes `frame` upright and writes it under `home/Pictures`.
pub fn save_still(home: &Path, frame: &CameraFrame, stem: &str) -> Result<Saved, String> {
    frame.validate().map_err(|e| e.to_string())?;
    let dir = pictures_dir(home);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create Pictures: {e}"))?;
    let name = free_name(&dir, stem);
    let (width, height, rgb) = upright_pixels(frame, false, false);
    let part = dir.join(format!(".{name}.part"));
    let written = (|| -> Result<(), String> {
        let file = fs::File::create(&part).map_err(|e| e.to_string())?;
        let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
        writer.write_image_data(&rgb).map_err(|e| e.to_string())?;
        writer.finish().map_err(|e| e.to_string())?;
        fs::rename(&part, dir.join(&name)).map_err(|e| e.to_string())
    })();
    if let Err(error) = written {
        let _ = fs::remove_file(&part);
        return Err(error);
    }
    Ok(Saved {
        name,
        width,
        height,
    })
}

/// `$HOME`, or a typed reason.
pub fn home_from_env() -> Result<PathBuf, String> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::frame;
    use ferrix_hardware_api::CameraRotation;

    #[test]
    fn stills_save_upright_as_png_with_unique_names() {
        let home = std::env::temp_dir().join(format!("ferrix-camera-store-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        let still = frame("cam-b", CameraRotation::Deg90, 1);
        let first = save_still(&home, &still, "IMG_20261124_094100").unwrap();
        assert_eq!(first.name, "IMG_20261124_094100.png");
        assert_eq!((first.width, first.height), (2, 3), "rotated upright");
        let second = save_still(&home, &still, "IMG_20261124_094100").unwrap();
        assert_eq!(second.name, "IMG_20261124_094100_1.png");
        let decoder =
            png::Decoder::new(fs::File::open(pictures_dir(&home).join(&first.name)).unwrap());
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (2, 3));
        let leftovers: Vec<_> = fs::read_dir(pictures_dir(&home))
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .collect();
        assert!(leftovers.is_empty());
        let _ = fs::remove_dir_all(&home);
    }

    #[test]
    fn stems_use_local_time() {
        let zoned: jiff::Zoned = "2026-11-24T09:41:00[UTC]".parse().unwrap();
        assert_eq!(still_stem(&zoned), "IMG_20261124_094100");
    }
}
