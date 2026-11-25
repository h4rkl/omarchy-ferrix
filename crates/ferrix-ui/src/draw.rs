//! Thin drawing layer over `tiny-skia` (anti-aliased CPU 2D rendering) plus
//! the text path in [`crate::text`] (`cosmic-text` shaping, bidi and
//! fallback, `swash` rasterisation). Reuses popular libraries per AGENTS.md;
//! only the small glue is project code. Software rendering is intentional.

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Stroke, Transform};

pub use crate::text::{Face, Style, Type, Weight};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub struct Canvas {
    pm: Pixmap,
}

fn paint(color: u32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8((color >> 16) as u8, (color >> 8) as u8, color as u8, 255);
    p.anti_alias = true;
    p
}

fn rounded_path(r: Rect, radius: i32) -> Option<tiny_skia::Path> {
    let (x, y, w, h) = (r.x as f32, r.y as f32, r.w as f32, r.h as f32);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let rad = (radius as f32).clamp(0.0, (w / 2.0).min(h / 2.0));
    let mut pb = PathBuilder::new();
    pb.move_to(x + rad, y);
    pb.line_to(x + w - rad, y);
    pb.quad_to(x + w, y, x + w, y + rad);
    pb.line_to(x + w, y + h - rad);
    pb.quad_to(x + w, y + h, x + w - rad, y + h);
    pb.line_to(x + rad, y + h);
    pb.quad_to(x, y + h, x, y + h - rad);
    pb.line_to(x, y + rad);
    pb.quad_to(x, y, x + rad, y);
    pb.close();
    pb.finish()
}

impl Canvas {
    pub fn new(width: i32, height: i32, fill: u32) -> Self {
        let mut pm = Pixmap::new(width.max(1) as u32, height.max(1) as u32)
            .expect("canvas dimensions must be valid");
        pm.fill(tiny_skia::Color::from_rgba8(
            (fill >> 16) as u8,
            (fill >> 8) as u8,
            fill as u8,
            255,
        ));
        Self { pm }
    }

    pub fn width(&self) -> i32 {
        self.pm.width() as i32
    }

    pub fn height(&self) -> i32 {
        self.pm.height() as i32
    }

    /// Packed 0xRRGGBB at a pixel, or 0 outside the canvas. Used by tests and
    /// by preview inspection.
    pub fn get(&self, x: i32, y: i32) -> u32 {
        if x < 0 || y < 0 || x >= self.width() || y >= self.height() {
            return 0;
        }
        let i = ((y * self.width() + x) * 4) as usize;
        let d = self.pm.data();
        u32::from(d[i]) << 16 | u32::from(d[i + 1]) << 8 | u32::from(d[i + 2])
    }

    pub fn put(&mut self, x: i32, y: i32, color: u32) {
        if x < 0 || y < 0 || x >= self.width() || y >= self.height() {
            return;
        }
        let i = ((y * self.width() + x) * 4) as usize;
        let d = self.pm.data_mut();
        d[i] = (color >> 16) as u8;
        d[i + 1] = (color >> 8) as u8;
        d[i + 2] = color as u8;
        d[i + 3] = 255;
    }

    /// Blend `color` over the existing pixel with coverage `alpha` (0..=255).
    pub fn blend(&mut self, x: i32, y: i32, color: u32, alpha: u8) {
        if x < 0 || y < 0 || x >= self.width() || y >= self.height() || alpha == 0 {
            return;
        }
        let i = ((y * self.width() + x) * 4) as usize;
        let a = u32::from(alpha);
        let inv = 255 - a;
        let d = self.pm.data_mut();
        let mix = |bg: u8, fg: u32| ((u32::from(bg) * inv + fg * a) / 255) as u8;
        d[i] = mix(d[i], color >> 16 & 0xff);
        d[i + 1] = mix(d[i + 1], color >> 8 & 0xff);
        d[i + 2] = mix(d[i + 2], color & 0xff);
        d[i + 3] = 255;
    }

    pub fn fill_rect(&mut self, r: Rect, color: u32) {
        let Some(rect) = tiny_skia::Rect::from_xywh(r.x as f32, r.y as f32, r.w as f32, r.h as f32)
        else {
            return;
        };
        self.pm
            .fill_rect(rect, &paint(color), Transform::identity(), None);
    }

    pub fn fill_rounded(&mut self, r: Rect, radius: i32, color: u32) {
        let Some(path) = rounded_path(r, radius) else {
            return;
        };
        self.pm.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    pub fn stroke_rounded(&mut self, r: Rect, radius: i32, thickness: i32, color: u32) {
        let Some(path) = rounded_path(r, radius) else {
            return;
        };
        let stroke = Stroke {
            width: thickness as f32,
            ..Stroke::default()
        };
        self.pm
            .stroke_path(&path, &paint(color), &stroke, Transform::identity(), None);
    }

    pub fn fill_circle(&mut self, cx: i32, cy: i32, radius: i32, color: u32) {
        let mut pb = PathBuilder::new();
        pb.push_circle(cx as f32, cy as f32, radius as f32);
        let Some(path) = pb.finish() else {
            return;
        };
        self.pm.fill_path(
            &path,
            &paint(color),
            FillRule::Winding,
            Transform::identity(),
            None,
        );
    }

    /// Draws a straight-alpha RGBA8 image of `width` x `height` pixels
    /// scaled (bicubic) into `dest`, blended over the canvas. Returns false
    /// and draws nothing when the buffer does not match the size.
    pub fn draw_rgba(&mut self, dest: Rect, width: u32, height: u32, rgba: &[u8]) -> bool {
        if dest.w <= 0
            || dest.h <= 0
            || width == 0
            || height == 0
            || rgba.len() as u64 != u64::from(width) * u64::from(height) * 4
        {
            return false;
        }
        // tiny-skia pixmaps are premultiplied.
        let premultiplied: Vec<u8> = rgba
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|&[r, g, b, a]| {
                let p = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
                [p(r), p(g), p(b), a]
            })
            .collect();
        let Some(image) = tiny_skia::IntSize::from_wh(width, height)
            .and_then(|size| Pixmap::from_vec(premultiplied, size))
        else {
            return false;
        };
        let transform = Transform::from_row(
            dest.w as f32 / width as f32,
            0.0,
            0.0,
            dest.h as f32 / height as f32,
            dest.x as f32,
            dest.y as f32,
        );
        let paint = tiny_skia::PixmapPaint {
            quality: tiny_skia::FilterQuality::Bicubic,
            ..tiny_skia::PixmapPaint::default()
        };
        self.pm
            .draw_pixmap(0, 0, image.as_ref(), &paint, transform, None);
        true
    }

    /// Little-endian XRGB8888 bytes as expected by `wl_shm` format `xrgb8888`.
    pub fn xrgb_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pm.data().len());
        for px in self.pm.data().as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], 0xff]);
        }
        out
    }

    /// Binary PPM (P6) bytes, for the cross-platform `--preview` path.
    pub fn ppm_bytes(&self) -> Vec<u8> {
        let mut out = format!("P6\n{} {}\n255\n", self.width(), self.height()).into_bytes();
        for px in self.pm.data().as_chunks::<4>().0 {
            out.extend_from_slice(&px[..3]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgba_images_scale_into_the_destination_and_blend() {
        let mut c = Canvas::new(40, 40, 0x000000);
        // 2x2: opaque red, transparent, transparent, half-alpha white.
        let rgba = [
            255, 0, 0, 255, 0, 0, 0, 0, //
            0, 0, 0, 0, 255, 255, 255, 128,
        ];
        let dest = Rect {
            x: 10,
            y: 10,
            w: 20,
            h: 20,
        };
        assert!(c.draw_rgba(dest, 2, 2, &rgba));
        assert_eq!(c.get(13, 13), 0xff0000, "top-left quadrant is red");
        assert_eq!(c.get(26, 13), 0x000000, "transparent keeps the canvas");
        let blended = c.get(26, 26);
        assert!((0x70..=0x90).contains(&(blended & 0xff)), "{blended:06x}");
        assert_eq!(c.get(5, 5), 0x000000, "nothing outside the destination");
        // Mismatched buffers draw nothing.
        assert!(!c.draw_rgba(dest, 3, 2, &rgba));
        assert!(!c.draw_rgba(Rect { w: 0, ..dest }, 2, 2, &rgba));
    }
}
