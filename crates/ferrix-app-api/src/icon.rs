//! Bounded launcher bitmaps for installed apps (ADR-008, "Installed APKs").
//!
//! A runtime adapter may provide the launcher icon of an app it reports by
//! itself (an APK). The icon crosses the `ferrix_appd` protocol as a small
//! straight-alpha RGBA8 bitmap, already decoded and downscaled by the
//! adapter, so the home surface never parses an image format or reads a
//! runtime's files. Every constructor (including deserialization) enforces
//! the size bound, so a peer cannot make a client allocate more than
//! [`APP_ICON_MAX_SIDE`]² pixels.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Largest width or height of an [`AppIconBitmap`], in pixels: the home
/// grid tile glyph box (96 design px). Larger icons are downscaled by the
/// adapter; the home scales to its draw size.
pub const APP_ICON_MAX_SIDE: u32 = 96;

/// A launcher icon bitmap: `width` x `height` straight (non-premultiplied)
/// RGBA8 pixels, row-major, both sides in `1..=APP_ICON_MAX_SIDE`.
///
/// Wire form: `{"width":96,"height":96,"rgba":"<base64, standard alphabet>"}`.
#[derive(Clone, PartialEq, Eq)]
pub struct AppIconBitmap {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

impl AppIconBitmap {
    /// Checks the bounds and the buffer length.
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        if width == 0 || height == 0 || width > APP_ICON_MAX_SIDE || height > APP_ICON_MAX_SIDE {
            return Err(format!(
                "icon {width}x{height} outside 1..={APP_ICON_MAX_SIDE} pixels per side"
            ));
        }
        let expected = (width * height * 4) as usize;
        if rgba.len() != expected {
            return Err(format!(
                "icon {width}x{height} needs {expected} RGBA bytes, got {}",
                rgba.len()
            ));
        }
        Ok(Self {
            width,
            height,
            rgba,
        })
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    /// Straight-alpha RGBA8 pixels, row-major.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Whether every pixel is fully transparent (nothing to draw).
    pub fn is_blank(&self) -> bool {
        self.rgba.as_chunks::<4>().0.iter().all(|px| px[3] == 0)
    }
}

impl fmt::Debug for AppIconBitmap {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AppIconBitmap")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    width: u32,
    height: u32,
    rgba: String,
}

impl Serialize for AppIconBitmap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        Wire {
            width: self.width,
            height: self.height,
            rgba: STANDARD.encode(&self.rgba),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for AppIconBitmap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let wire = Wire::deserialize(deserializer)?;
        // Refuse an oversized payload before decoding it.
        let max = (APP_ICON_MAX_SIDE * APP_ICON_MAX_SIDE * 4) as usize;
        if wire.rgba.len() > max.div_ceil(3) * 4 {
            return Err(D::Error::custom("icon payload larger than the icon bound"));
        }
        let rgba = STANDARD
            .decode(wire.rgba.as_bytes())
            .map_err(|e| D::Error::custom(format!("icon rgba: {e}")))?;
        AppIconBitmap::new(wire.width, wire.height, rgba).map_err(D::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounds_and_lengths_are_enforced() {
        assert!(AppIconBitmap::new(2, 2, vec![0; 16]).is_ok());
        assert!(AppIconBitmap::new(0, 2, vec![]).is_err());
        assert!(AppIconBitmap::new(2, 2, vec![0; 15]).is_err());
        let side = APP_ICON_MAX_SIDE + 1;
        assert!(AppIconBitmap::new(side, 1, vec![0; side as usize * 4]).is_err());
        let full = APP_ICON_MAX_SIDE;
        assert!(AppIconBitmap::new(full, full, vec![0; (full * full * 4) as usize]).is_ok());
    }

    #[test]
    fn wire_form_round_trips_as_base64() {
        let icon = AppIconBitmap::new(1, 2, vec![1, 2, 3, 255, 4, 5, 6, 0]).unwrap();
        let text = serde_json::to_string(&icon).unwrap();
        assert_eq!(text, r#"{"width":1,"height":2,"rgba":"AQID/wQFBgA="}"#);
        let back: AppIconBitmap = serde_json::from_str(&text).unwrap();
        assert_eq!(back, icon);
        assert!(!back.is_blank());
        assert!(
            AppIconBitmap::new(1, 1, vec![9, 9, 9, 0])
                .unwrap()
                .is_blank()
        );
    }

    #[test]
    fn malformed_or_oversized_wire_icons_are_rejected() {
        for bad in [
            r#"{"width":1,"height":1,"rgba":"AQID"}"#,
            r#"{"width":1,"height":1,"rgba":"!!!!"}"#,
            r#"{"width":0,"height":1,"rgba":""}"#,
            r#"{"width":1,"height":1,"rgba":"AQID/w==","extra":1}"#,
        ] {
            assert!(serde_json::from_str::<AppIconBitmap>(bad).is_err(), "{bad}");
        }
        let huge = format!(
            r#"{{"width":96,"height":96,"rgba":"{}"}}"#,
            "A".repeat(200_000)
        );
        assert!(serde_json::from_str::<AppIconBitmap>(&huge).is_err());
    }
}
