//! Renders every Ferrix glyph at the status-bar, row and launcher sizes to a
//! PPM contact sheet for legibility review:
//!
//!   cargo run -p ferrix-ui --example icon-sheet -- out/icon-sheet.ppm [basis]
//!
//! `basis` is the panel's short side (default 1080; the QEMU agent panel is
//! 830). Sizes follow the 1080 px design basis: 30 px (status bar), 56 px
//! (rows and headers), 96 px (launcher tiles). Each glyph is shown in `fg` on
//! `bg_dark`, in `fg_dim` on `surface` (inactive), and in `rust` (active).

use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::icons::{self, Icon};
use ferrix_ui::theme;
use ferrix_ui::widgets::scale;

fn main() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or("usage: icon-sheet OUT.ppm [basis]")?;
    let basis: i32 = args
        .next()
        .map(|b| b.parse().map_err(|e| format!("basis: {e}")))
        .transpose()?
        .unwrap_or(1080);
    let t = theme::get();
    let ty = Type::load()?;
    let sizes = [scale(30, basis), scale(56, basis), scale(96, basis)];
    let row_h = scale(120, basis);
    let label_w = scale(300, basis);
    let col_w = scale(130, basis);
    let width = label_w + col_w * 5;
    let height = row_h * Icon::ALL.len() as i32;
    let mut c = Canvas::new(width, height, t.bg);
    let label = Style::regular(scale(22, basis) as f32, t.fg_dim);
    for (i, icon) in Icon::ALL.iter().enumerate() {
        let y = i as i32 * row_h;
        ty.text(
            &mut c,
            label,
            icon.source(),
            scale(12, basis),
            y + row_h / 2 + scale(8, basis),
        );
        let cells = [
            (sizes[0], t.fg, t.bg_dark),
            (sizes[1], t.fg, t.bg_dark),
            (sizes[2], t.fg, t.bg_dark),
            (sizes[2], t.fg_dim, t.surface),
            (sizes[2], t.rust, t.surface),
        ];
        for (j, (size, color, fill)) in cells.into_iter().enumerate() {
            let cell = Rect {
                x: label_w + j as i32 * col_w,
                y: y + scale(6, basis),
                w: col_w - scale(8, basis),
                h: row_h - scale(12, basis),
            };
            c.fill_rect(cell, fill);
            icons::draw_in(&mut c, *icon, cell, size, color);
        }
    }
    std::fs::write(&out, c.ppm_bytes()).map_err(|e| format!("{out}: {e}"))?;
    println!("{out} {width}x{height} basis {basis}");
    Ok(())
}
