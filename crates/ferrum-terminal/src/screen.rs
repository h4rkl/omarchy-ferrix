//! Composition of the Terminal surface: a real shell's grid above the touch
//! keyboard, in Ferrum's Tokyo Night look with the purple accent of the
//! launcher tile. The status indicators (battery, radio) belong to the home
//! surface; the terminal draws only the clock strip so the header geometry
//! matches the rest of the reference surfaces.

use ferrum_ui::draw::{Canvas, Rect, Style, Type};
use ferrum_ui::osk::{self, Key};
use ferrum_ui::theme;

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
    let mut c = Canvas::new(w, h, theme::BG);
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
    let w = c.width();
    c.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w,
            h: bar_h,
        },
        theme::BG_DARK,
    );
    c.fill_rect(
        Rect {
            x: 0,
            y: bar_h,
            w,
            h: 2,
        },
        theme::BORDER,
    );
    let px = (bar_h * 5 / 12) as f32;
    ty.text(c, Style::bold(px, theme::FG), clock, pad, bar_h * 2 / 3);
}

fn gesture_pill(c: &mut Canvas, state: &UiState) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    c.fill_rounded(
        Rect {
            x: (c.width() - s(320)) / 2,
            y: c.height() - s(60),
            w: s(320),
            h: if state.pressed == Some(Target::Home) {
                s(18)
            } else {
                s(12)
            },
        },
        s(9),
        if state.pressed == Some(Target::Home) {
            theme::BLUE
        } else {
            theme::FG_DIM
        },
    );
}

fn page_header(c: &mut Canvas, ty: &Type, title: &str, state: &UiState) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let back = Rect {
        x: s(36),
        y: s(106),
        w: s(160),
        h: s(104),
    };
    if state.pressed == Some(Target::Back) {
        c.fill_rounded(back, s(22), theme::SURFACE_HI);
    }
    ty.text(
        c,
        Style::bold(s(48) as f32, theme::BLUE),
        "<",
        s(64),
        s(178),
    );
    ty.text(
        c,
        Style::bold(s(54) as f32, theme::FG),
        title,
        s(150),
        s(178),
    );
    c.fill_rect(
        Rect {
            x: s(48),
            y: s(230),
            w: c.width() - s(96),
            h: s(2).max(1),
        },
        theme::BORDER,
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
    let style = Style::regular(px, theme::FG);
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

fn terminal_page(c: &mut Canvas, ty: &Type, state: &UiState, term: Option<&Snapshot>) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let m = terminal_metrics(ty, c.width() as u32, c.height() as u32);
    c.fill_rounded(m.area, s(16), theme::BG_DARK);
    c.stroke_rounded(m.area, s(16), s(2).max(1), theme::BORDER);
    let regular = Style::regular(m.px, theme::FG);
    let ascent = ty.ascent(regular);
    let cell_rect = |col: usize, row: usize| Rect {
        x: m.origin.0 + col as i32 * m.cell_w,
        y: m.origin.1 + row as i32 * m.cell_h,
        w: m.cell_w,
        h: m.cell_h,
    };
    match term {
        None => {
            let style = Style::regular(s(30) as f32, theme::FG_DIM);
            let msg = "Starting shell...";
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
            if let Some((col, row)) = snap.cursor
                && col < cols
                && row < rows
            {
                let r = cell_rect(col, row);
                let color = if snap.exited.is_some() {
                    theme::FG_DIM
                } else {
                    theme::PURPLE
                };
                c.fill_rect(r, color);
                if let Some(cell) = snap.cell(col, row)
                    && cell.c != ' '
                {
                    ty.char(
                        c,
                        Style::regular(m.px, theme::BG_DARK),
                        cell.c,
                        r.x,
                        r.y + ascent,
                    );
                }
            }
            if snap.scrollback > 0 {
                let badge = format!("{} lines up", snap.scrollback);
                let style = Style::bold(s(24) as f32, theme::FG_DIM);
                ty.text(
                    c,
                    style,
                    &badge,
                    m.area.x + m.area.w - ty.width(style, &badge) - s(24),
                    m.area.y + s(40),
                );
            }
            if let Some(reason) = &snap.exited {
                let note = format!("[shell {reason}]  tap \u{23ce} to restart");
                let style = Style::bold(s(26) as f32, theme::YELLOW);
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
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
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
        theme::BG_DARK,
    );
    c.fill_rect(
        Rect {
            x: 0,
            y: tray_top,
            w: c.width(),
            h: s(2).max(1),
        },
        theme::BORDER,
    );
    for (r, key) in osk::keys(size, &state.osk) {
        let pressed = state.pressed == Some(Target::Key(key));
        let active = (key == Key::Shift && state.osk.shift) || (key == Key::Ctrl && state.osk.ctrl);
        let function = osk::is_function(key);
        let fill = if pressed {
            theme::SURFACE_HI
        } else if active {
            theme::PURPLE
        } else if function {
            theme::BG
        } else {
            theme::SURFACE
        };
        c.fill_rounded(r, s(14), fill);
        if pressed {
            c.stroke_rounded(r, s(14), s(3), theme::PURPLE);
        }
        let label = osk::label(key, &state.osk);
        if label.is_empty() {
            continue;
        }
        let color = if active {
            theme::BG_DARK
        } else if key == Key::Enter {
            theme::PURPLE
        } else if function {
            theme::FG_DIM
        } else {
            theme::FG
        };
        let px = s(if label.chars().count() > 1 { 26 } else { 40 }) as f32;
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
        let ty = Type::load().expect("embedded fonts load");
        let state = UiState::new();
        let m = terminal_metrics(&ty, 1080, 2340);
        assert!(m.cols >= 50 && m.rows >= 25, "{}x{}", m.cols, m.rows);
        let snap = Snapshot::from_bytes(m.cols, m.rows, b"root@ferrix:~# \x1b[42m  \x1b[0m");
        let c = compose(&ty, 1080, 2340, 600, &state, Some(&snap));
        assert_eq!((c.width(), c.height()), (1080, 2340));
        // Clock strip, grid panel background, a green cell background, and
        // the block cursor.
        assert_eq!(c.get(540, 10), theme::BG_DARK);
        let blank = (
            m.origin.0 + 30 * m.cell_w + 2,
            m.origin.1 + 5 * m.cell_h + 2,
        );
        assert_eq!(c.get(blank.0, blank.1), theme::BG_DARK);
        let green = (m.origin.0 + 16 * m.cell_w + 2, m.origin.1 + 2);
        assert_eq!(c.get(green.0, green.1), 0x9ece6a);
        let cursor = snap.cursor.expect("cursor visible");
        let cx = m.origin.0 + cursor.0 as i32 * m.cell_w + 2;
        let cy = m.origin.1 + cursor.1 as i32 * m.cell_h + 2;
        assert_eq!(c.get(cx, cy), theme::PURPLE);
        // Prompt glyphs leave non-background pixels in the first cell row.
        let first_row = (0..m.cell_w * 16).any(|dx| {
            (0..m.cell_h).any(|dy| c.get(m.origin.0 + dx, m.origin.1 + dy) != theme::BG_DARK)
        });
        assert!(first_row, "prompt text rendered");
        // Keys are drawn on the keyboard tray with the standard surface.
        let keys = osk::keys((1080, 2340), &state.osk);
        let (q, _) = keys.iter().find(|(_, k)| *k == Key::Char('q')).unwrap();
        assert_eq!(c.get(q.x + 6, q.y + 6), theme::SURFACE);
        let (enter, _) = keys.iter().find(|(_, k)| *k == Key::Enter).unwrap();
        assert_eq!(c.get(enter.x + 6, enter.y + 6), theme::BG);

        // A failed session renders its reason instead of a grid; a missing
        // one shows the starting note.
        let failed = Snapshot::failed("pty: denied".to_owned());
        let c = compose(&ty, 1080, 2340, 600, &state, Some(&failed));
        assert_eq!(c.get(blank.0, blank.1), theme::BG_DARK);
        let c = compose(&ty, 1080, 2340, 600, &state, None);
        assert_eq!(c.get(blank.0, blank.1), theme::BG_DARK);
    }

    #[test]
    fn pressed_back_and_active_modifiers_show_feedback() {
        let ty = Type::load().expect("embedded fonts load");
        let mut state = UiState::new();
        state.pressed = Some(Target::Back);
        state.osk.ctrl = true;
        let c = compose(&ty, 1080, 2340, 600, &state, None);
        assert_eq!(c.get(60, 130), theme::SURFACE_HI, "back highlight");
        let keys = osk::keys((1080, 2340), &state.osk);
        let (ctrl, _) = keys.iter().find(|(_, k)| *k == Key::Ctrl).unwrap();
        assert_eq!(c.get(ctrl.x + 6, ctrl.y + 6), theme::PURPLE);
    }
}
