//! Composition of the basic screen. Omarchy look and feel (Tokyo Night palette,
//! monospace type, flat bordered surfaces); GrapheneOS-style status bar,
//! minimal launcher grid, and bottom gesture pill where mobile UX is needed.
//! This is a QEMU reference surface rendered from the mock profile, not the
//! product shell.

use crate::draw::{Canvas, Rect, Style, Type};
use crate::interaction::{AppId, Page, Target, UiState};
use crate::osk::{self, Key};
use crate::profile::Profile;
use crate::terminal::Snapshot;
use crate::theme;

/// Scale a 1080-wide design length. Multiply first: `width / 1080` truncates
/// to 0 on the 830-wide QEMU panel and used to jump the status bar.
fn scale(v: i32, width: i32) -> i32 {
    v * width / 1080
}

pub fn compose(
    p: &Profile,
    minute_of_day: u32,
    ty: &Type,
    width: u32,
    height: u32,
    state: &UiState,
    term: Option<&Snapshot>,
) -> Canvas {
    let w = width as i32;
    let h = height as i32;
    let s = |v: i32| scale(v, w);
    let mut c = Canvas::new(w, h, theme::BG);
    let hour = minute_of_day / 60;
    let clock = format!("{:02}:{:02}", hour, minute_of_day % 60);

    if state.page != Page::Launcher {
        shell_page(&mut c, p, ty, &clock, state, term);
        gesture_pill(&mut c, state);
        return c;
    }

    status_bar(&mut c, p, ty, &clock, s(72), s(40), state);

    // Large clock and greeting, GrapheneOS lock-screen style.
    let clock_style = Style::bold(s(170) as f32, theme::FG);
    let cw = ty.width(clock_style, &clock);
    ty.text(&mut c, clock_style, &clock, (w - cw) / 2, s(430));
    let greeting = match hour {
        5..=11 => "Good morning",
        12..=17 => "Good afternoon",
        _ => "Good evening",
    };
    let greet_style = Style::regular(s(42) as f32, theme::FG_DIM);
    let gw = ty.width(greet_style, greeting);
    ty.text(&mut c, greet_style, greeting, (w - gw) / 2, s(520));

    // Battery and network cards from the mock profile.
    let margin = s(48);
    let gap = s(24);
    let card_w = (w - 2 * margin - gap) / 2;
    let card_h = s(300);
    let card_y = s(620);
    battery_card(
        &mut c,
        p,
        ty,
        Rect {
            x: margin,
            y: card_y,
            w: card_w,
            h: card_h,
        },
        state.pressed == Some(Target::Battery),
    );
    network_card(
        &mut c,
        p,
        ty,
        Rect {
            x: margin + card_w + gap,
            y: card_y,
            w: card_w,
            h: card_h,
        },
        state.pressed == Some(Target::Network),
    );

    launcher_grid(&mut c, ty, margin, gap, s(1080), state);

    // Honest footer label and GrapheneOS-style gesture pill.
    let caption = "QEMU reference surface - Rust basic screen";
    let cap_style = Style::regular(s(26) as f32, theme::FG_DIM);
    let capw = ty.width(cap_style, caption);
    ty.text(&mut c, cap_style, caption, (w - capw) / 2, h - s(150));
    gesture_pill(&mut c, state);
    c
}

fn gesture_pill(c: &mut Canvas, state: &UiState) {
    let width = c.width();
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

fn status_bar(
    c: &mut Canvas,
    p: &Profile,
    ty: &Type,
    clock: &str,
    bar_h: i32,
    pad: i32,
    state: &UiState,
) {
    let w = c.width();
    c.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w,
            h: bar_h,
        },
        if state.pressed == Some(Target::Status) {
            theme::SURFACE_HI
        } else {
            theme::BG_DARK
        },
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
    let baseline = bar_h * 2 / 3;
    // Clock on the left, GrapheneOS style.
    ty.text(c, Style::bold(px, theme::FG), clock, pad, baseline);

    // Right-aligned: signal bars, wifi arcs, battery percent and gauge.
    let mid = bar_h / 2;
    let pct = format!("{}%", p.battery_percent);
    let batt_color = if p.charging {
        theme::GREEN
    } else if p.battery_percent < 20 {
        theme::RED
    } else {
        theme::FG
    };
    let mut x = w - pad;
    x -= battery_icon(c, x, mid, p, batt_color);
    let pct_style = Style::regular(px, theme::FG);
    x -= ty.width(pct_style, &pct) + bar_h / 4;
    ty.text(c, pct_style, &pct, x, baseline);
    x -= bar_h / 3;
    x -= wifi_icon(c, x, mid + bar_h / 4, bar_h / 2, p.wifi == "connected");
    x -= bar_h / 3;
    signal_bars(c, x, mid + bar_h / 4, bar_h / 2, p.modem == "registered");
}

/// Draws a battery outline with proportional fill, right edge at `right`.
/// Returns the consumed width.
fn battery_icon(c: &mut Canvas, right: i32, mid: i32, p: &Profile, color: u32) -> i32 {
    let bw = 46;
    let bh = 24;
    let x = right - bw;
    let y = mid - bh / 2;
    c.stroke_rounded(
        Rect {
            x,
            y,
            w: bw - 4,
            h: bh,
        },
        5,
        3,
        theme::FG_DIM,
    );
    c.fill_rounded(
        Rect {
            x: x + bw - 4,
            y: mid - 5,
            w: 4,
            h: 10,
        },
        2,
        theme::FG_DIM,
    );
    let inner = (bw - 12) * i32::from(p.battery_percent.min(100)) / 100;
    if inner > 0 {
        c.fill_rounded(
            Rect {
                x: x + 4,
                y: y + 4,
                w: inner,
                h: bh - 8,
            },
            2,
            color,
        );
    }
    bw
}

/// Draws three wifi arcs opening upward with the apex at (right - size, base).
/// Returns the consumed width.
fn wifi_icon(c: &mut Canvas, right: i32, base: i32, size: i32, connected: bool) -> i32 {
    let cx = right - size;
    let color = if connected { theme::FG } else { theme::FG_DIM };
    for (i, r) in [size / 4, size * 5 / 8, size].into_iter().enumerate() {
        let on = connected || i == 0;
        for dy in -r - 2..=0 {
            for dx in -r - 2..=r + 2 {
                let d2 = dx * dx + dy * dy;
                if d2 >= (r - 2) * (r - 2) && d2 <= (r + 2) * (r + 2) && dx.abs() <= -dy {
                    c.put(
                        cx + dx,
                        base + dy,
                        if on { color } else { theme::SURFACE_HI },
                    );
                }
            }
        }
    }
    c.fill_circle(cx, base, 4, color);
    2 * size
}

fn signal_bars(c: &mut Canvas, right: i32, base: i32, size: i32, registered: bool) {
    let bw = size / 4;
    let gap = 4;
    for i in 0..4 {
        let bh = size * (i + 1) / 4 + 4;
        let on = registered || i == 0;
        c.fill_rounded(
            Rect {
                x: right - (4 - i) * (bw + gap),
                y: base - bh,
                w: bw,
                h: bh,
            },
            2,
            if on { theme::FG } else { theme::SURFACE_HI },
        );
    }
}

fn card(c: &mut Canvas, ty: &Type, r: Rect, title: &str, pressed: bool) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    c.fill_rounded(
        r,
        s(24),
        if pressed {
            theme::SURFACE_HI
        } else {
            theme::SURFACE
        },
    );
    c.stroke_rounded(
        r,
        s(24),
        if pressed { s(5) } else { 2 },
        if pressed { theme::BLUE } else { theme::BORDER },
    );
    ty.text(
        c,
        Style::regular(s(28) as f32, theme::FG_DIM),
        title,
        r.x + s(32),
        r.y + s(62),
    );
}

fn battery_card(c: &mut Canvas, p: &Profile, ty: &Type, r: Rect, pressed: bool) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    card(c, ty, r, "BATTERY", pressed);
    let color = if p.charging {
        theme::GREEN
    } else if p.battery_percent < 20 {
        theme::RED
    } else {
        theme::BLUE
    };
    ty.text(
        c,
        Style::bold(s(72) as f32, color),
        &format!("{}%", p.battery_percent),
        r.x + s(32),
        r.y + s(160),
    );
    let track = Rect {
        x: r.x + s(32),
        y: r.y + s(190),
        w: r.w - s(64),
        h: s(14),
    };
    c.fill_rounded(track, s(7), theme::SURFACE_HI);
    let fill_w = track.w * i32::from(p.battery_percent.min(100)) / 100;
    if fill_w > 0 {
        c.fill_rounded(
            Rect {
                x: track.x,
                y: track.y,
                w: fill_w,
                h: track.h,
            },
            s(7),
            color,
        );
    }
    let status = if p.charging {
        "Charging - mock"
    } else {
        "Discharging - mock"
    };
    ty.text(
        c,
        Style::regular(s(26) as f32, theme::FG_DIM),
        status,
        r.x + s(32),
        r.y + s(258),
    );
}

fn network_card(c: &mut Canvas, p: &Profile, ty: &Type, r: Rect, pressed: bool) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    card(c, ty, r, "NETWORK", pressed);
    let connected = p.wifi == "connected";
    ty.text(
        c,
        Style::bold(
            s(48) as f32,
            if connected {
                theme::CYAN
            } else {
                theme::FG_DIM
            },
        ),
        "Wi-Fi",
        r.x + s(32),
        r.y + s(140),
    );
    let wifi_line = if connected {
        "Connected"
    } else {
        p.wifi.as_str()
    };
    ty.text(
        c,
        Style::regular(s(30) as f32, theme::FG),
        wifi_line,
        r.x + s(32),
        r.y + s(196),
    );
    let cell = format!(
        "LTE {} - SIM {}",
        if p.modem == "registered" {
            "ok"
        } else {
            p.modem.as_str()
        },
        p.sim
    );
    ty.text(
        c,
        Style::regular(s(26) as f32, theme::FG_DIM),
        &cell,
        r.x + s(32),
        r.y + s(258),
    );
}

fn launcher_grid(c: &mut Canvas, ty: &Type, margin: i32, gap: i32, top: i32, state: &UiState) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    let apps: [(AppId, u32); 6] = [
        (AppId::Settings, theme::BLUE),
        (AppId::Messages, theme::GREEN),
        (AppId::Terminal, theme::PURPLE),
        (AppId::Dialer, theme::CYAN),
        (AppId::Camera, theme::YELLOW),
        (AppId::Files, theme::ORANGE),
    ];
    let tile = (c.width() - 2 * margin - 2 * gap) / 3;
    for (i, (app, accent)) in apps.into_iter().enumerate() {
        let label = app.label();
        let col = (i % 3) as i32;
        let row = (i / 3) as i32;
        let r = Rect {
            x: margin + col * (tile + gap),
            y: top + row * (tile + gap),
            w: tile,
            h: tile,
        };
        let pressed = state.pressed == Some(Target::App(app));
        c.fill_rounded(
            r,
            s(28),
            if pressed {
                theme::SURFACE_HI
            } else {
                theme::SURFACE
            },
        );
        c.stroke_rounded(
            r,
            s(28),
            if pressed { s(5) } else { 2 },
            if pressed { accent } else { theme::BORDER },
        );
        let icx = r.x + r.w / 2;
        let icy = r.y + s(118);
        app_icon(c, ty, label, accent, icx, icy);
        let label_style = Style::regular(s(30) as f32, theme::FG);
        let lw = ty.width(label_style, label);
        ty.text(c, label_style, label, icx - lw / 2, r.y + r.h - s(44));
    }
}

fn app_icon(c: &mut Canvas, ty: &Type, label: &str, accent: u32, cx: i32, cy: i32) {
    let width = c.width();
    let u = |v: i32| scale(v, width);
    match label {
        "Settings" => {
            // Sliders: three rails with offset knobs.
            for (i, knob) in [-u(24), u(20), -u(4)].into_iter().enumerate() {
                let y = cy - u(28) + i as i32 * u(28);
                c.fill_rounded(
                    Rect {
                        x: cx - u(44),
                        y: y - u(4),
                        w: u(88),
                        h: u(8),
                    },
                    u(4),
                    theme::SURFACE_HI,
                );
                c.fill_circle(cx + knob, y, u(12), accent);
            }
        }
        "Messages" => {
            c.fill_rounded(
                Rect {
                    x: cx - u(44),
                    y: cy - u(36),
                    w: u(88),
                    h: u(64),
                },
                u(16),
                accent,
            );
            for i in 0..3 {
                c.fill_rounded(
                    Rect {
                        x: cx - u(28),
                        y: cy - u(22) + i * u(16),
                        w: u(56) - i * u(14),
                        h: u(6),
                    },
                    u(3),
                    theme::BG,
                );
            }
        }
        "Terminal" => {
            c.stroke_rounded(
                Rect {
                    x: cx - u(46),
                    y: cy - u(38),
                    w: u(92),
                    h: u(72),
                },
                u(12),
                u(4),
                accent,
            );
            ty.text(
                c,
                Style::bold(u(36) as f32, accent),
                ">_",
                cx - u(24),
                cy + u(14),
            );
        }
        "Dialer" => {
            for row in 0..3 {
                for col in 0..3 {
                    c.fill_circle(
                        cx - u(28) + col * u(28),
                        cy - u(28) + row * u(28),
                        u(9),
                        accent,
                    );
                }
            }
        }
        "Camera" => {
            c.stroke_rounded(
                Rect {
                    x: cx - u(46),
                    y: cy - u(34),
                    w: u(92),
                    h: u(68),
                },
                u(14),
                u(4),
                accent,
            );
            c.fill_circle(cx, cy, u(18), accent);
            c.fill_circle(cx + u(30), cy - u(20), u(4), accent);
        }
        _ => {
            // Files: folder body with tab.
            c.fill_rounded(
                Rect {
                    x: cx - u(44),
                    y: cy - u(34),
                    w: u(40),
                    h: u(14),
                },
                u(5),
                accent,
            );
            c.fill_rounded(
                Rect {
                    x: cx - u(44),
                    y: cy - u(26),
                    w: u(88),
                    h: u(58),
                },
                u(9),
                accent,
            );
        }
    }
}

fn shell_page(
    c: &mut Canvas,
    p: &Profile,
    ty: &Type,
    clock: &str,
    state: &UiState,
    term: Option<&Snapshot>,
) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    status_bar(c, p, ty, clock, s(72), s(40), state);
    match state.page {
        Page::Launcher => {}
        Page::App(AppId::Terminal) => terminal_page(c, ty, state, term),
        Page::App(app) => app_page(c, ty, app, state),
        Page::QuickSettings => quick_settings(c, ty, state),
        Page::Notifications => notifications(c, ty, state),
    }
}

fn page_header(c: &mut Canvas, ty: &Type, title: &str, state: &UiState) {
    let width = c.width();
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

fn app_page(c: &mut Canvas, ty: &Type, app: AppId, state: &UiState) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    page_header(c, ty, app.label(), state);
    let (accent, eyebrow, title, detail) = match app {
        AppId::Settings => (
            theme::BLUE,
            "DEVICE",
            "Settings are ready",
            "Display, sound, connectivity, and privacy controls use mock state in QEMU.",
        ),
        AppId::Messages => (
            theme::GREEN,
            "INBOX",
            "No conversations",
            "New messages will appear here when the messaging service is connected.",
        ),
        AppId::Terminal => (
            theme::PURPLE,
            "SHELL",
            "guest@omarchy:~$",
            "The mandatory serial console remains available in the host terminal.",
        ),
        AppId::Dialer => (
            theme::CYAN,
            "CALLS",
            "Mock modem registered",
            "Calling is unavailable in this generic QEMU target.",
        ),
        AppId::Camera => (
            theme::YELLOW,
            "CAMERA",
            "No camera device",
            "The generic QEMU target does not claim camera hardware support.",
        ),
        AppId::Files => (
            theme::ORANGE,
            "FILES",
            "Temporary guest session",
            "Generated artifacts stay on the host under out/qemu-aarch64.",
        ),
    };
    c.fill_rounded(
        Rect {
            x: s(72),
            y: s(360),
            w: c.width() - s(144),
            h: s(560),
        },
        s(34),
        theme::SURFACE,
    );
    c.stroke_rounded(
        Rect {
            x: s(72),
            y: s(360),
            w: c.width() - s(144),
            h: s(560),
        },
        s(34),
        s(3),
        accent,
    );
    ty.text(
        c,
        Style::bold(s(30) as f32, accent),
        eyebrow,
        s(120),
        s(450),
    );
    ty.text(
        c,
        Style::bold(s(58) as f32, theme::FG),
        title,
        s(120),
        s(570),
    );
    wrap_text(c, ty, detail, s(120), s(675), c.width() - s(240), s(34));
    ty.text(
        c,
        Style::regular(s(26) as f32, theme::FG_DIM),
        "Swipe from the left edge to go back",
        s(120),
        s(850),
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

/// The Terminal app: a real shell's grid above the touch keyboard. Flat
/// Omarchy surfaces, the launcher tile's purple as the single accent.
fn terminal_page(c: &mut Canvas, ty: &Type, state: &UiState, term: Option<&Snapshot>) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    page_header(c, ty, "Terminal", state);
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
    let width = c.width();
    let s = |v: i32| scale(v, width);
    let size = (c.width() as u32, c.height() as u32);
    let (top, bottom) = osk::extent(size);
    let tray_top = top - s(12);
    c.fill_rect(
        Rect {
            x: 0,
            y: tray_top,
            w: width,
            h: bottom - tray_top,
        },
        theme::BG_DARK,
    );
    c.fill_rect(
        Rect {
            x: 0,
            y: tray_top,
            w: width,
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

fn quick_settings(c: &mut Canvas, ty: &Type, state: &UiState) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    page_header(c, ty, "Quick settings", state);
    toggle(
        c,
        ty,
        Rect {
            x: s(86),
            y: s(468),
            w: s(420),
            h: s(300),
        },
        "Wi-Fi",
        state.wifi_on,
        state.pressed == Some(Target::Wifi),
        theme::CYAN,
    );
    toggle(
        c,
        ty,
        Rect {
            x: s(574),
            y: s(468),
            w: s(420),
            h: s(300),
        },
        "Bluetooth",
        state.bluetooth_on,
        state.pressed == Some(Target::Bluetooth),
        theme::BLUE,
    );
    ty.text(
        c,
        Style::bold(s(30) as f32, theme::FG_DIM),
        "BRIGHTNESS",
        s(96),
        s(970),
    );
    let track = Rect {
        x: s(130),
        y: s(1084),
        w: s(820),
        h: s(28),
    };
    c.fill_rounded(track, s(14), theme::SURFACE_HI);
    let fill = track.w * i32::from(state.brightness) / 100;
    c.fill_rounded(Rect { w: fill, ..track }, s(14), theme::YELLOW);
    c.fill_circle(track.x + fill, track.y + track.h / 2, s(34), theme::FG);
    let value = format!("{}%", state.brightness);
    ty.text(
        c,
        Style::bold(s(34) as f32, theme::FG),
        &value,
        s(96),
        s(1210),
    );
    ty.text(
        c,
        Style::regular(s(27) as f32, theme::FG_DIM),
        "Drag the rail to adjust the QEMU/mock display value",
        s(96),
        s(1280),
    );
}

fn toggle(c: &mut Canvas, ty: &Type, r: Rect, label: &str, on: bool, pressed: bool, accent: u32) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    let color = if on { accent } else { theme::SURFACE };
    c.fill_rounded(r, s(32), if pressed { theme::SURFACE_HI } else { color });
    c.stroke_rounded(
        r,
        s(32),
        if pressed { s(6) } else { s(3) },
        if on { accent } else { theme::BORDER },
    );
    ty.text(
        c,
        Style::bold(s(44) as f32, theme::FG),
        label,
        r.x + s(38),
        r.y + s(104),
    );
    ty.text(
        c,
        Style::regular(
            s(30) as f32,
            if on { theme::BG_DARK } else { theme::FG_DIM },
        ),
        if on { "On" } else { "Off" },
        r.x + s(38),
        r.y + s(190),
    );
}

fn notifications(c: &mut Canvas, ty: &Type, state: &UiState) {
    let width = c.width();
    let s = |v: i32| scale(v, width);
    page_header(c, ty, "Notifications", state);
    if !state.notification_visible {
        let title = "You're all caught up";
        let style = Style::bold(s(48) as f32, theme::FG);
        ty.text(
            c,
            style,
            title,
            (c.width() - ty.width(style, title)) / 2,
            s(780),
        );
        let detail = "Swipe up from the bottom edge to return Home.";
        let detail_style = Style::regular(s(28) as f32, theme::FG_DIM);
        ty.text(
            c,
            detail_style,
            detail,
            (c.width() - ty.width(detail_style, detail)) / 2,
            s(860),
        );
        return;
    }
    let card = Rect {
        x: s(72),
        y: s(390),
        w: c.width() - s(144),
        h: s(390),
    };
    c.fill_rounded(card, s(34), theme::SURFACE);
    c.stroke_rounded(card, s(34), s(2).max(1), theme::BORDER);
    ty.text(
        c,
        Style::bold(s(30) as f32, theme::BLUE),
        "SYSTEM",
        s(116),
        s(475),
    );
    ty.text(
        c,
        Style::bold(s(48) as f32, theme::FG),
        "QEMU session ready",
        s(116),
        s(575),
    );
    wrap_text(
        c,
        ty,
        "Touch input is active. Swipe up from the bottom edge at any time to return Home.",
        s(116),
        s(665),
        c.width() - s(232),
        s(30),
    );
    let clear = Rect {
        x: s(270),
        y: s(1685),
        w: s(540),
        h: s(170),
    };
    c.fill_rounded(
        clear,
        s(30),
        if state.pressed == Some(Target::ClearNotifications) {
            theme::SURFACE_HI
        } else {
            theme::SURFACE
        },
    );
    c.stroke_rounded(clear, s(30), s(3), theme::BORDER);
    let label = "Clear all";
    let style = Style::bold(s(38) as f32, theme::FG);
    ty.text(
        c,
        style,
        label,
        (c.width() - ty.width(style, label)) / 2,
        s(1790),
    );
}

fn wrap_text(c: &mut Canvas, ty: &Type, text: &str, x: i32, y: i32, max_width: i32, px: i32) {
    let style = Style::regular(px as f32, theme::FG_DIM);
    let mut line = String::new();
    let mut baseline = y;
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && ty.width(style, &candidate) > max_width {
            ty.text(c, style, &line, x, baseline);
            line = word.to_owned();
            baseline += px * 3 / 2;
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        ty.text(c, style, &line, x, baseline);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile {
        Profile {
            schema_version: 1,
            name: "emulator-dev".into(),
            display_width: 1080,
            display_height: 2340,
            battery_percent: 75,
            charging: false,
            wifi: "connected".into(),
            bluetooth_present: true,
            modem: "registered".into(),
            sim: "present".into(),
        }
    }

    #[test]
    fn composes_non_blank_portrait_surface() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let state = UiState::new(true, true);
        let c = compose(
            &p,
            9 * 60 + 41,
            &ty,
            p.display_width,
            p.display_height,
            &state,
            None,
        );
        assert_eq!((c.width(), c.height()), (1080, 2340));
        // Status bar background and main background are themed.
        assert_eq!(c.get(540, 10), theme::BG_DARK);
        assert_eq!(c.get(540, 600), theme::BG);
        // The rendered frame is not blank: some pixel differs from the ground.
        let bytes = c.xrgb_bytes();
        assert!(bytes.chunks(4).any(|q| {
            let v = u32::from_le_bytes([q[0], q[1], q[2], q[3]]) & 0xff_ffff;
            v != theme::BG && v != theme::BG_DARK
        }));
    }

    #[test]
    fn every_interactive_page_renders() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        for page in [
            Page::App(AppId::Settings),
            Page::App(AppId::Messages),
            Page::App(AppId::Terminal),
            Page::App(AppId::Dialer),
            Page::App(AppId::Camera),
            Page::App(AppId::Files),
            Page::QuickSettings,
            Page::Notifications,
        ] {
            let mut state = UiState::new(true, true);
            state.page = page;
            let c = compose(&p, 600, &ty, 1080, 2340, &state, None);
            assert_eq!((c.width(), c.height()), (1080, 2340));
            assert_ne!(c.get(300, 500), theme::BG);
        }
    }

    #[test]
    fn terminal_page_renders_grid_cursor_and_keyboard() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let mut state = UiState::new(true, true);
        state.page = Page::App(AppId::Terminal);
        let m = terminal_metrics(&ty, 1080, 2340);
        assert!(m.cols >= 50 && m.rows >= 25, "{}x{}", m.cols, m.rows);
        let snap = Snapshot::from_bytes(m.cols, m.rows, b"root@omarchy:~# \x1b[42m  \x1b[0m");
        let c = compose(&p, 600, &ty, 1080, 2340, &state, Some(&snap));
        // Grid panel background, a green cell background, and the block cursor.
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
            (0..m.cell_h).any(|dy| {
                let v = c.get(m.origin.0 + dx, m.origin.1 + dy);
                v != theme::BG_DARK
            })
        });
        assert!(first_row, "prompt text rendered");
        // Keys are drawn on the keyboard tray with the standard surface.
        let keys = osk::keys((1080, 2340), &state.osk);
        let (q, _) = keys.iter().find(|(_, k)| *k == Key::Char('q')).unwrap();
        assert_eq!(c.get(q.x + 6, q.y + 6), theme::SURFACE);
        let (enter, _) = keys.iter().find(|(_, k)| *k == Key::Enter).unwrap();
        assert_eq!(c.get(enter.x + 6, enter.y + 6), theme::BG);

        // A failed session renders its reason instead of a grid.
        let failed = Snapshot::failed("pty: denied".to_owned());
        let c = compose(&p, 600, &ty, 1080, 2340, &state, Some(&failed));
        assert_eq!(c.get(blank.0, blank.1), theme::BG_DARK);
    }

    #[test]
    fn status_bar_height_stays_put_when_the_shade_opens() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        // Include the Cocoa QEMU panel (830) where the old unit scale truncated to 1
        // and jumped the bar from 55px on the launcher to 72px on notifications.
        for width in [830_u32, 1080, 1200] {
            let height = width * 2340 / 1080;
            let launcher = compose(&p, 600, &ty, width, height, &UiState::new(true, true), None);
            let mut shade_state = UiState::new(true, true);
            shade_state.page = Page::Notifications;
            let shade = compose(&p, 600, &ty, width, height, &shade_state, None);
            let mut quick_state = UiState::new(true, true);
            quick_state.page = Page::QuickSettings;
            let quick = compose(&p, 600, &ty, width, height, &quick_state, None);

            let bar_h = scale(72, width as i32);
            let x = width as i32 / 2;
            for frame in [&launcher, &shade, &quick] {
                assert_eq!(frame.get(x, 0), theme::BG_DARK, "width={width}");
                assert_eq!(frame.get(x, bar_h - 1), theme::BG_DARK, "width={width}");
                assert_eq!(frame.get(x, bar_h), theme::BORDER, "width={width}");
            }
        }
    }
}
