//! Composition of the Terminal surface: a real shell's grid above the touch
//! keyboard, in the Omarchy Ferrix visual system (`docs/design.md`): slate
//! surfaces, a `bg_dark` grid well and keyboard tray, hairline borders, and
//! a sharp `rust` action accent and quieter `rust_dim` structural rules. The
//! block cursor, sticky modifier, pressed outline, and back glyph use rust;
//! rules carry oxidized orange without glare.
//! The status indicators (battery, radio) belong to the home surface; the
//! terminal draws only the clock strip so the header geometry matches the rest
//! of the reference surfaces. Colors come from [`theme::get`]; geometry used
//! by hit tests lives in `ferrix_ui::osk` and is not restyled here.

use ferrix_ui::draw::{Canvas, Rect, Style, Type};
use ferrix_ui::osk::{self, Key};
use ferrix_ui::theme;

use crate::interaction::{Target, UiState};
use crate::terminal::Snapshot;

/// Scale a 1080-wide design length. Multiply first so narrow panels never
/// truncate to zero.
fn scale(v: i32, width: i32) -> i32 {
    v * width / 1080
}

/// Stable density basis for both orientations.
fn design_width(c: &Canvas) -> i32 {
    c.width().min(c.height())
}

/// Hairline thickness: 2 px at the 1080 design width, never less than 1.
fn hairline(width: i32) -> i32 {
    scale(2, width).max(1)
}

pub fn compose(
    ty: &Type,
    width: u32,
    height: u32,
    minute_of_day: u32,
    state: &UiState,
    term: Option<&Snapshot>,
) -> Canvas {
    let w = width as i32;
    let h = height as i32;
    let mut c = Canvas::new(w, h, theme::get().bg);
    let clock = format!("{:02}:{:02}", minute_of_day / 60, minute_of_day % 60);
    let basis = w.min(h);
    let s = |v: i32| scale(v, basis);
    clock_strip(&mut c, ty, &clock, s(72), s(40));
    page_header(&mut c, ty, "Terminal", state);
    terminal_page(&mut c, ty, state, term);
    gesture_pill(&mut c, state);
    c
}

/// The top strip: same height and colours as the home status bar, clock only.
fn clock_strip(c: &mut Canvas, ty: &Type, clock: &str, bar_h: i32, pad: i32) {
    let t = theme::get();
    let w = c.width();
    c.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w,
            h: bar_h,
        },
        t.bg_dark,
    );
    c.fill_rect(
        Rect {
            x: 0,
            y: bar_h,
            w,
            h: hairline(design_width(c)),
        },
        t.rust_dim,
    );
    let px = (bar_h * 5 / 12) as f32;
    ty.text(c, Style::bold(px, t.fg), clock, pad, bar_h * 2 / 3);
}

/// The compositor home indicator: the one pill shape in the system.
fn gesture_pill(c: &mut Canvas, state: &UiState) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let pressed = state.pressed == Some(Target::Home);
    c.fill_rounded(
        Rect {
            x: (c.width() - s(320)) / 2,
            y: c.height() - s(60),
            w: s(320),
            h: if pressed { s(18) } else { s(12) },
        },
        s(9),
        if pressed { t.steel } else { t.fg_dim },
    );
}

fn page_header(c: &mut Canvas, ty: &Type, title: &str, state: &UiState) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    // The back hit rect (`interaction.rs`) is unchanged; only its corner
    // radius follows the 8 px card radius.
    let back = Rect {
        x: s(36),
        y: s(106),
        w: s(160),
        h: s(104),
    };
    if state.pressed == Some(Target::Back) {
        c.fill_rounded(back, s(8), t.surface_hi);
    }
    ty.text(c, Style::bold(s(48) as f32, t.rust), "<", s(64), s(178));
    ty.text(c, Style::bold(s(54) as f32, t.fg), title, s(150), s(178));
    c.fill_rect(
        Rect {
            x: s(48),
            y: s(230),
            w: c.width() - s(96),
            h: hairline(width),
        },
        t.rust_dim,
    );
}

/// Terminal grid geometry derived from the panel size and the monospace face.
pub struct TermMetrics {
    pub area: Rect,
    pub origin: (i32, i32),
    pub px: f32,
    pub cell_w: i32,
    pub cell_h: i32,
    pub cols: usize,
    pub rows: usize,
}

pub fn terminal_metrics(ty: &Type, width: u32, height: u32) -> TermMetrics {
    let w = width as i32;
    let area = osk::terminal_area((width, height));
    let px = (scale(30, w) as f32).max(12.0);
    let style = Style::regular(px, theme::get().fg);
    let cell_w = (ty.advance(style, 'M').round() as i32).max(1);
    let cell_h = ty.line_height(style).max(1);
    let pad = scale(16, w);
    let cols = ((area.w - 2 * pad) / cell_w).max(2) as usize;
    let rows = ((area.h - 2 * pad) / cell_h).max(1) as usize;
    TermMetrics {
        area,
        origin: (area.x + pad, area.y + pad),
        px,
        cell_w,
        cell_h,
        cols,
        rows,
    }
}

/// Small uppercase mono chrome label, right-aligned to `right` on `baseline`.
fn chrome_label_right(c: &mut Canvas, ty: &Type, text: &str, right: i32, baseline: i32, px: i32) {
    let style = Style::bold(px as f32, theme::get().fg_dim);
    ty.text(c, style, text, right - ty.width(style, text), baseline);
}

fn terminal_page(c: &mut Canvas, ty: &Type, state: &UiState, term: Option<&Snapshot>) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let m = terminal_metrics(ty, c.width() as u32, c.height() as u32);
    c.fill_rounded(m.area, s(8), t.bg_dark);
    c.stroke_rounded(m.area, s(8), hairline(width), t.border);
    let regular = Style::regular(m.px, t.fg);
    let ascent = ty.ascent(regular);
    let cell_rect = |col: usize, row: usize| Rect {
        x: m.origin.0 + col as i32 * m.cell_w,
        y: m.origin.1 + row as i32 * m.cell_h,
        w: m.cell_w,
        h: m.cell_h,
    };
    match term {
        None => {
            let style = Style::regular(s(30) as f32, t.fg_dim);
            let msg = "shell · starting";
            ty.text(
                c,
                style,
                msg,
                m.area.x + (m.area.w - ty.width(style, msg)) / 2,
                m.area.y + m.area.h / 2,
            );
        }
        Some(snap) => {
            let cols = snap.cols.min(m.cols);
            let rows = snap.rows.min(m.rows);
            for row in 0..rows {
                for col in 0..cols {
                    let Some(cell) = snap.cell(col, row) else {
                        continue;
                    };
                    let r = cell_rect(col, row);
                    if let Some(bg) = cell.bg {
                        c.fill_rect(r, bg);
                    }
                    if cell.c != ' ' {
                        let style = if cell.bold {
                            Style::bold(m.px, cell.fg)
                        } else {
                            Style::regular(m.px, cell.fg)
                        };
                        ty.char(c, style, cell.c, r.x, r.y + ascent);
                    }
                }
            }
            // The block cursor is the grid's one accent element; it dims once
            // the shell is gone.
            if let Some((col, row)) = snap.cursor
                && col < cols
                && row < rows
            {
                let r = cell_rect(col, row);
                let color = if snap.exited.is_some() {
                    t.fg_dim
                } else {
                    t.rust
                };
                c.fill_rect(r, color);
                if let Some(cell) = snap.cell(col, row)
                    && cell.c != ' '
                {
                    ty.char(
                        c,
                        Style::regular(m.px, t.bg_dark),
                        cell.c,
                        r.x,
                        r.y + ascent,
                    );
                }
            }
            if snap.scrollback > 0 {
                let badge = format!("SCROLLBACK {}", snap.scrollback);
                chrome_label_right(
                    c,
                    ty,
                    &badge,
                    m.area.x + m.area.w - s(24),
                    m.area.y + s(40),
                    s(24),
                );
            }
            if let Some(reason) = &snap.exited {
                let note = format!("shell · {reason} · Enter restarts");
                let style = Style::bold(s(26) as f32, t.warn);
                ty.text(
                    c,
                    style,
                    &note,
                    m.area.x + s(24),
                    m.area.y + m.area.h - s(24),
                );
            }
        }
    }
    keyboard(c, ty, state);
}

fn keyboard(c: &mut Canvas, ty: &Type, state: &UiState) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let hair = hairline(width);
    let size = (c.width() as u32, c.height() as u32);
    let (top, bottom) = osk::extent(size);
    let tray_top = top - s(12);
    c.fill_rect(
        Rect {
            x: 0,
            y: tray_top,
            w: c.width(),
            h: bottom - tray_top,
        },
        t.bg_dark,
    );
    c.fill_rect(
        Rect {
            x: 0,
            y: tray_top,
            w: c.width(),
            h: hair,
        },
        t.border,
    );
    for (r, key) in osk::keys(size, &state.osk) {
        let pressed = state.pressed == Some(Target::Key(key));
        let active =
            (key == Key::Shift && state.osk.shift_active()) || (key == Key::Ctrl && state.osk.ctrl);
        let function = osk::is_function(key);
        let fill = if pressed {
            t.surface_hi
        } else if active {
            t.rust
        } else if function {
            t.bg
        } else {
            t.surface
        };
        // Key rects are the hit-test rects from `osk::keys`; only the radius
        // (6 px chip) and the hairline are styling.
        c.fill_rounded(r, s(6), fill);
        let outline = if pressed { t.rust } else { t.border };
        c.stroke_rounded(r, s(6), hair, outline);
        let label = osk::label(key, &state.osk);
        if label.is_empty() {
            continue;
        }
        // Chrome labels (esc, tab, ctrl, ?123, abc) read as uppercase mono;
        // character keys keep their own case so shift stays legible.
        let label = if function {
            label.to_uppercase()
        } else {
            label
        };
        let color = if active {
            t.bg_dark
        } else if function {
            t.fg_dim
        } else {
            t.fg
        };
        let px = s(if label.chars().count() > 1 { 24 } else { 40 }) as f32;
        let style = if function {
            Style::bold(px, color)
        } else {
            Style::regular(px, color)
        };
        let lw = ty.width(style, &label);
        let baseline = r.y + r.h / 2 + (px * 0.36) as i32;
        ty.text(c, style, &label, r.x + (r.w - lw) / 2, baseline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_page_renders_grid_cursor_and_keyboard() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let state = UiState::new();
        let m = terminal_metrics(&ty, 1080, 2340);
        assert!(m.cols >= 50 && m.rows >= 25, "{}x{}", m.cols, m.rows);
        let snap = Snapshot::from_bytes(m.cols, m.rows, b"root@ferrix:~# \x1b[42m  \x1b[0m");
        let c = compose(&ty, 1080, 2340, 600, &state, Some(&snap));
        assert_eq!((c.width(), c.height()), (1080, 2340));
        // Clock strip, grid well background, an ANSI green cell background,
        // and the rust block cursor.
        assert_eq!(c.get(540, 10), t.bg_dark);
        let blank = (
            m.origin.0 + 30 * m.cell_w + 2,
            m.origin.1 + 5 * m.cell_h + 2,
        );
        assert_eq!(c.get(blank.0, blank.1), t.bg_dark);
        let green = (m.origin.0 + 16 * m.cell_w + 2, m.origin.1 + 2);
        assert_eq!(c.get(green.0, green.1), t.ansi[2]);
        let cursor = snap.cursor.expect("cursor visible");
        let cx = m.origin.0 + cursor.0 as i32 * m.cell_w + 2;
        let cy = m.origin.1 + cursor.1 as i32 * m.cell_h + 2;
        assert_eq!(c.get(cx, cy), t.rust);
        // Prompt glyphs leave non-background pixels in the first cell row.
        let first_row = (0..m.cell_w * 16)
            .any(|dx| (0..m.cell_h).any(|dy| c.get(m.origin.0 + dx, m.origin.1 + dy) != t.bg_dark));
        assert!(first_row, "prompt text rendered");
        // Keys are drawn on the keyboard tray: characters on `surface`,
        // function keys on `bg`.
        let keys = osk::keys((1080, 2340), &state.osk);
        let (q, _) = keys.iter().find(|(_, k)| *k == Key::Char('q')).unwrap();
        assert_eq!(c.get(q.x + 6, q.y + 6), t.surface);
        let (enter, _) = keys.iter().find(|(_, k)| *k == Key::Enter).unwrap();
        assert_eq!(c.get(enter.x + 6, enter.y + 6), t.bg);

        // A failed session renders its reason instead of a grid; a missing
        // one shows the starting note.
        let failed = Snapshot::failed("pty: denied".to_owned());
        let c = compose(&ty, 1080, 2340, 600, &state, Some(&failed));
        assert_eq!(c.get(blank.0, blank.1), t.bg_dark);
        let c = compose(&ty, 1080, 2340, 600, &state, None);
        assert_eq!(c.get(blank.0, blank.1), t.bg_dark);
    }

    #[test]
    fn exited_cursor_drops_the_accent() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let state = UiState::new();
        let m = terminal_metrics(&ty, 1080, 2340);
        let mut snap = Snapshot::from_bytes(m.cols, m.rows, b"$ exit\r\n");
        snap.exited = Some("exit code 0".to_owned());
        let c = compose(&ty, 1080, 2340, 600, &state, Some(&snap));
        let cursor = snap.cursor.expect("cursor visible");
        let cx = m.origin.0 + cursor.0 as i32 * m.cell_w + 2;
        let cy = m.origin.1 + cursor.1 as i32 * m.cell_h + 2;
        assert_eq!(c.get(cx, cy), t.fg_dim);
    }

    #[test]
    fn pressed_back_and_active_modifiers_show_feedback() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let mut state = UiState::new();
        state.pressed = Some(Target::Back);
        state.osk.ctrl = true;
        let c = compose(&ty, 1080, 2340, 600, &state, None);
        assert_eq!(c.get(60, 130), t.surface_hi, "back highlight");
        let keys = osk::keys((1080, 2340), &state.osk);
        let (ctrl, _) = keys.iter().find(|(_, k)| *k == Key::Ctrl).unwrap();
        assert_eq!(c.get(ctrl.x + 6, ctrl.y + 6), t.rust);
    }
}
