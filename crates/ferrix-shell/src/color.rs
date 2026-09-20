//! The one color the compositor paints itself: the output clear color under
//! every client surface. It comes from the shared Ferrix theme
//! (`ferrix_ui::theme`, loaded from `colors.toml` at startup), so the
//! compositor background and the surfaces drawn on it are one palette.
//! Everything else on screen belongs to clients.

#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

/// The active theme's `bg` as `[r, g, b]` channels in `0.0..=1.0`, the form
/// the renderers' clear color takes. Ferrix black `#000000` unless a loaded
/// `colors.toml` says otherwise.
pub fn clear_rgb() -> [f32; 3] {
    rgb_channels(ferrix_ui::theme::get().bg)
}

/// Splits a `0xRRGGBB` theme value into unit-range channels, without gamma
/// conversion: the renderers take the same 8-bit-per-channel values the theme
/// file specifies.
pub fn rgb_channels(rgb: u32) -> [f32; 3] {
    let [_, r, g, b] = rgb.to_be_bytes();
    [r, g, b].map(|channel| f32::from(channel) / 255.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferrix_ui::theme::Theme;

    fn close(actual: [f32; 3], expected: [f32; 3]) -> bool {
        actual
            .iter()
            .zip(expected)
            .all(|(a, e)| (a - e).abs() < 1e-6)
    }

    #[test]
    fn channels_split_rrggbb_without_gamma() {
        assert_eq!(rgb_channels(0x000000), [0.0, 0.0, 0.0]);
        assert_eq!(rgb_channels(0xffffff), [1.0, 1.0, 1.0]);
        assert!(close(rgb_channels(0xff8000), [1.0, 128.0 / 255.0, 0.0]));
    }

    #[test]
    fn clear_color_is_the_ferrix_background() {
        // No test in this binary calls `theme::init`, so `get()` is the built-in
        // palette, which the canonical `themes/ferrix/colors.toml` must equal.
        let bg = Theme::ferrix().bg;
        assert_eq!(bg, 0x000000, "Ferrix black bg");
        assert_eq!(clear_rgb(), rgb_channels(bg));
        assert!(close(clear_rgb(), [0.0, 0.0, 0.0]));
    }
}
