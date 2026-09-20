//! Composition of the basic screen. Omarchy Ferrix look and feel (Tokyo Night palette,
//! monospace type, flat bordered surfaces); GrapheneOS-style status bar,
//! minimal launcher grid, and bottom gesture pill where mobile UX is needed.
//! This is a QEMU reference surface rendered from the mock profile, not the
//! product shell.

use ferrix_hardware_api::{HardwareState, ModemRegistration, WifiState};
use ferrix_ui::draw::{Canvas, Rect, Style, Type, Weight};
use ferrix_ui::theme;

use crate::catalog::{Tile, tone_color};
use crate::interaction::{Page, Target, UiState, tile_rects};

/// Scale a 1080-wide design length. Multiply first: `width / 1080` truncates
/// to 0 on the 830-wide QEMU panel and used to jump the status bar.
fn scale(v: i32, width: i32) -> i32 {
    v * width / 1080
}

/// Stable density basis for both orientations. Using raw canvas width makes
/// every control more than twice as large after a portrait panel rotates.
fn design_width(c: &Canvas) -> i32 {
    c.width().min(c.height())
}

pub fn compose(
    p: &HardwareState,
    minute_of_day: u32,
    ty: &Type,
    width: u32,
    height: u32,
    state: &UiState,
) -> Canvas {
    let w = width as i32;
    let h = height as i32;
    let basis = w.min(h);
    let s = |v: i32| scale(v, basis);
    let mut c = Canvas::new(w, h, theme::BG);
    let hour = minute_of_day / 60;
    let clock = format!("{:02}:{:02}", hour, minute_of_day % 60);

    if state.page != Page::Launcher {
        shell_page(&mut c, p, ty, &clock, state);
        gesture_pill(&mut c, state);
        return c;
    }

    if w > h {
        launcher_landscape(&mut c, p, ty, &clock, state);
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

    if let Some(notice) = &state.crash_notice {
        crash_banner(
            &mut c,
            ty,
            Rect {
                x: margin,
                y: s(944),
                w: w - 2 * margin,
                h: s(112),
            },
            &notice.name,
            &notice.reason,
        );
    }
    launcher_grid(&mut c, ty, state);

    // Honest footer label and GrapheneOS-style gesture pill.
    let caption = "QEMU reference surface - Rust basic screen";
    let cap_style = Style::regular(s(26) as f32, theme::FG_DIM);
    let capw = ty.width(cap_style, caption);
    ty.text(&mut c, cap_style, caption, (w - capw) / 2, h - s(150));
    gesture_pill(&mut c, state);
    c
}

fn launcher_landscape(c: &mut Canvas, p: &HardwareState, ty: &Type, clock: &str, state: &UiState) {
    let basis = design_width(c);
    let s = |v: i32| scale(v, basis);
    status_bar(c, p, ty, clock, s(72), s(40), state);

    let clock_style = Style::bold(s(150) as f32, theme::FG);
    let clock_width = ty.width(clock_style, clock);
    ty.text(c, clock_style, clock, s(460) - clock_width / 2, s(280));
    let greeting = "Omarchy Ferrix mobile";
    let greeting_style = Style::regular(s(38) as f32, theme::FG_DIM);
    let greeting_width = ty.width(greeting_style, greeting);
    ty.text(
        c,
        greeting_style,
        greeting,
        s(460) - greeting_width / 2,
        s(350),
    );

    let card_y = s(430);
    let card_width = s(400);
    let card_height = s(260);
    battery_card(
        c,
        p,
        ty,
        Rect {
            x: s(40),
            y: card_y,
            w: card_width,
            h: card_height,
        },
        state.pressed == Some(Target::Battery),
    );
    network_card(
        c,
        p,
        ty,
        Rect {
            x: s(460),
            y: card_y,
            w: card_width,
            h: card_height,
        },
        state.pressed == Some(Target::Network),
    );
    if let Some(notice) = &state.crash_notice {
        crash_banner(
            c,
            ty,
            Rect {
                x: s(40),
                y: s(720),
                w: s(820),
                h: s(96),
            },
            &notice.name,
            &notice.reason,
        );
    }
    launcher_grid(c, ty, state);
    gesture_pill(c, state);
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

fn status_bar(
    c: &mut Canvas,
    p: &HardwareState,
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
    let pct = format!("{}%", p.power.battery_percent);
    let batt_color = if p.power.charging {
        theme::GREEN
    } else if p.power.battery_percent < 20 {
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
    x -= wifi_icon(
        c,
        x,
        mid + bar_h / 4,
        bar_h / 2,
        p.network.wifi == WifiState::Connected,
    );
    x -= bar_h / 3;
    signal_bars(
        c,
        x,
        mid + bar_h / 4,
        bar_h / 2,
        p.telephony.registration == ModemRegistration::Registered,
    );
}

/// Draws a battery outline with proportional fill, right edge at `right`.
/// Returns the consumed width.
fn battery_icon(c: &mut Canvas, right: i32, mid: i32, p: &HardwareState, color: u32) -> i32 {
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
    let inner = (bw - 12) * i32::from(p.power.battery_percent.min(100)) / 100;
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
    if p.power.charging {
        bolt(c, x + (bw - 4) / 2, mid, bh - 8, theme::BG_DARK);
    }
    bw
}

/// A small filled lightning bolt centred on (`cx`, `cy`) with the given
/// height: the GrapheneOS-style charging cue. Classic seven-point outline,
/// scan-line filled so it stays solid at status-bar sizes.
fn bolt(c: &mut Canvas, cx: i32, cy: i32, h: i32, color: u32) {
    let h = h.max(6) as f32;
    let w = h * 0.62;
    let x0 = cx as f32 - w / 2.0;
    let y0 = cy as f32 - h / 2.0;
    const OUTLINE: [(f32, f32); 7] = [
        (0.62, 0.0),
        (0.12, 0.58),
        (0.46, 0.58),
        (0.34, 1.0),
        (0.90, 0.40),
        (0.54, 0.40),
        (0.70, 0.0),
    ];
    let pts: Vec<(f32, f32)> = OUTLINE
        .iter()
        .map(|(px, py)| (x0 + px * w, y0 + py * h))
        .collect();
    let top = y0.floor() as i32;
    let bottom = (y0 + h).ceil() as i32;
    for y in top..=bottom {
        let sy = y as f32 + 0.5;
        let mut xs = Vec::new();
        for i in 0..pts.len() {
            let (ax, ay) = pts[i];
            let (bx, by) = pts[(i + 1) % pts.len()];
            if (ay <= sy && by > sy) || (by <= sy && ay > sy) {
                xs.push(ax + (sy - ay) * (bx - ax) / (by - ay));
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        for pair in xs.chunks(2) {
            if let [l, r] = pair {
                for x in l.round() as i32..r.round() as i32 {
                    c.put(x, y, color);
                }
            }
        }
    }
}

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
    let width = design_width(c);
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

fn battery_card(c: &mut Canvas, p: &HardwareState, ty: &Type, r: Rect, pressed: bool) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    card(c, ty, r, "BATTERY", pressed);
    let color = if p.power.charging {
        theme::GREEN
    } else if p.power.battery_percent < 20 {
        theme::RED
    } else {
        theme::BLUE
    };
    ty.text(
        c,
        Style::bold(s(72) as f32, color),
        &format!("{}%", p.power.battery_percent),
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
    let fill_w = track.w * i32::from(p.power.battery_percent.min(100)) / 100;
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
    if p.power.charging {
        let pct_w = ty.width(
            Style::bold(s(72) as f32, color),
            &format!("{}%", p.power.battery_percent),
        );
        bolt(
            c,
            r.x + s(32) + pct_w + s(36),
            r.y + s(160) - s(26),
            s(52),
            color,
        );
    }
    let status = if p.power.charging {
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

fn network_card(c: &mut Canvas, p: &HardwareState, ty: &Type, r: Rect, pressed: bool) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    card(c, ty, r, "NETWORK", pressed);
    let connected = p.network.wifi == WifiState::Connected;
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
        p.network.wifi.name()
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
        if p.telephony.registration == ModemRegistration::Registered {
            "ok"
        } else {
            p.telephony.registration.name()
        },
        p.telephony.sim.name()
    );
    ty.text(
        c,
        Style::regular(s(26) as f32, theme::FG_DIM),
        &cell,
        r.x + s(32),
        r.y + s(258),
    );
}

/// The launcher grid: three columns, rows growing with the catalog. Tiles
/// share their rectangles with the hit test. A launched app shows a small
/// accent dot while it runs and a dimmed "Starting..." label until `ferrix-appd`
/// reports it in the foreground or background.
fn launcher_grid(c: &mut Canvas, ty: &Type, state: &UiState) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let size = (c.width() as u32, c.height() as u32);
    let landscape = c.width() > c.height();
    for (index, (tile, r)) in state
        .tiles
        .iter()
        .zip(tile_rects(size, state.tiles.len()))
        .enumerate()
    {
        if r.y >= c.height() {
            break;
        }
        let accent = tone_color(tile.icon.tone);
        let pressed = state.pressed == Some(Target::App(index));
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
        let icy = r.y + s(if landscape { 105 } else { 118 });
        app_icon(c, ty, tile, accent, icx, icy);
        let (label, color) = if tile.starting {
            ("Starting...".to_owned(), theme::FG_DIM)
        } else {
            (tile.name.clone(), theme::FG)
        };
        let label_style = Style::regular(s(if landscape { 28 } else { 30 }) as f32, color);
        let lw = ty.width(label_style, &label);
        ty.text(
            c,
            label_style,
            &label,
            icx - lw / 2,
            r.y + r.h - s(if landscape { 38 } else { 44 }),
        );
        if tile.running {
            c.fill_circle(r.x + r.w - s(34), r.y + s(34), s(9), accent);
        }
    }
}

/// Tile artwork. The reference entries keep their drawn icons; any other
/// app shows its descriptor glyph when the face has it, else the first
/// letter of its name (never a tofu box).
fn app_icon(c: &mut Canvas, ty: &Type, tile: &Tile, accent: u32, cx: i32, cy: i32) {
    let width = design_width(c);
    let u = |v: i32| scale(v, width);
    match tile.kind() {
        "settings" => {
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
        "messages" => {
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
        "terminal" => {
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
        "dialer" => {
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
        "camera" => {
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
        "files" => {
            // Folder body with tab.
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
        _ => {
            let glyph = tile_glyph(ty, tile);
            let style = Style::bold(
                u(if glyph.chars().count() > 1 { 40 } else { 56 }) as f32,
                accent,
            );
            let gw = ty.width(style, &glyph);
            ty.text(c, style, &glyph, cx - gw / 2, cy + u(20));
        }
    }
}

/// The descriptor glyph when every character exists in the bold face, else
/// the first letter of the name.
pub fn tile_glyph(ty: &Type, tile: &Tile) -> String {
    let glyph = tile.icon.glyph.trim();
    if ty.has_glyphs(Weight::Bold, glyph) {
        return glyph.to_owned();
    }
    tile.name
        .chars()
        .next()
        .map(|ch| ch.to_uppercase().to_string())
        .unwrap_or_else(|| "?".to_owned())
}

/// Restrained crash notice: a flat surface with a red accent bar and one
/// line of text. Dismissed by the next tap or by relaunching the app.
fn crash_banner(c: &mut Canvas, ty: &Type, r: Rect, name: &str, reason: &str) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    c.fill_rounded(r, s(20), theme::SURFACE);
    c.stroke_rounded(r, s(20), 2, theme::BORDER);
    c.fill_rounded(
        Rect {
            x: r.x + s(18),
            y: r.y + s(22),
            w: s(8),
            h: r.h - s(44),
        },
        s(4),
        theme::RED,
    );
    let text = format!("{name} stopped unexpectedly \u{b7} {reason}");
    let mut style = Style::regular(s(28) as f32, theme::FG);
    let max = r.w - s(72);
    while ty.width(style, &text) > max && style.px > 12.0 {
        style.px -= 2.0;
    }
    let baseline = r.y + r.h / 2 + (style.px * 0.36) as i32;
    ty.text(c, style, &text, r.x + s(48), baseline);
}

fn shell_page(c: &mut Canvas, p: &HardwareState, ty: &Type, clock: &str, state: &UiState) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    status_bar(c, p, ty, clock, s(72), s(40), state);
    match state.page {
        Page::Launcher => {}
        Page::Reference(index) => reference_page(c, ty, state.tiles.get(index), state),
        Page::QuickSettings => quick_settings(c, ty, state),
        Page::Notifications => notifications(c, ty, state),
    }
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

/// Illustrative content of a non-launchable catalog entry, keyed by the id's
/// last segment. Unknown entries get a generic "not installed" page.
fn reference_page(c: &mut Canvas, ty: &Type, tile: Option<&Tile>, state: &UiState) {
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let name = tile.map_or("Unknown", |t| t.name.as_str());
    page_header(c, ty, name, state);
    let fallback_title = format!("{name} is not installed");
    let (accent, eyebrow, title, detail) = match tile.map_or("", Tile::kind) {
        "settings" => (
            theme::BLUE,
            "DEVICE",
            "Settings are ready",
            "Display, sound, connectivity, and privacy controls use mock state in QEMU.",
        ),
        "messages" => (
            theme::GREEN,
            "INBOX",
            "No conversations",
            "New messages will appear here when the messaging service is connected.",
        ),
        "dialer" => (
            theme::CYAN,
            "CALLS",
            "Mock modem registered",
            "Calling is unavailable in this generic QEMU target.",
        ),
        "camera" => (
            theme::YELLOW,
            "CAMERA",
            "No camera device",
            "The generic QEMU target does not claim camera hardware support.",
        ),
        "files" => (
            theme::ORANGE,
            "FILES",
            "Temporary guest session",
            "Generated artifacts stay on the host under out/qemu-aarch64.",
        ),
        _ => (
            tile.map_or(theme::FG_DIM, |t| tone_color(t.icon.tone)),
            "APP",
            fallback_title.as_str(),
            "No runtime provides this application yet. The catalog entry is a placeholder.",
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

fn quick_settings(c: &mut Canvas, ty: &Type, state: &UiState) {
    let width = design_width(c);
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
    let width = design_width(c);
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
    let width = design_width(c);
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
    use crate::interaction::CrashNotice;
    use crate::test_tiles;

    /// The checked-in profile's initial hardware state, as the core would
    /// report it (mock values, not hardware facts).
    fn profile() -> HardwareState {
        crate::test_state()
    }

    fn state() -> UiState {
        UiState::new(true, true, test_tiles())
    }

    #[test]
    fn charging_changes_the_battery_indicator() {
        let ty = Type::load().expect("embedded fonts load");
        let mut p = profile();
        let state = state();
        let before = compose(&p, 600, &ty, 1080, 2340, &state);
        p.power.charging = true;
        let after = compose(&p, 600, &ty, 1080, 2340, &state);
        // The status-bar battery fill turns green and gains a bolt; the card
        // headline turns green too. Compare the card headline region.
        let card_y = scale(620, 1080) + scale(160, 1080) - scale(40, 1080);
        let card_x = scale(48, 1080) + scale(32, 1080) + scale(30, 1080);
        let mut differs = false;
        for dy in 0..scale(60, 1080) {
            for dx in 0..scale(200, 1080) {
                if before.get(card_x + dx, card_y + dy) != after.get(card_x + dx, card_y + dy) {
                    differs = true;
                }
            }
        }
        assert!(differs, "charging must be visible on the battery card");
        let bar_h = scale(72, 1080);
        let bolt_seen = (0..bar_h).any(|dy| {
            (1080 - scale(40, 1080) - 46..1080 - scale(40, 1080))
                .any(|x| after.get(x, dy) == theme::BG_DARK && before.get(x, dy) != theme::BG_DARK)
        });
        assert!(
            bolt_seen,
            "status-bar battery must show a bolt cut-out when charging"
        );
    }

    #[test]
    fn composes_non_blank_portrait_surface_with_catalog_tiles() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let state = state();
        assert_eq!(state.tiles.len(), 6);
        let c = compose(
            &p,
            9 * 60 + 41,
            &ty,
            p.display.width,
            p.display.height,
            &state,
        );
        assert_eq!((c.width(), c.height()), (1080, 2340));
        // Status bar background and main background are themed.
        assert_eq!(c.get(540, 10), theme::BG_DARK);
        assert_eq!(c.get(540, 600), theme::BG);
        // Every tile has its surface drawn.
        for r in tile_rects((1080, 2340), state.tiles.len()) {
            assert_eq!(c.get(r.x + r.w / 2, r.y + 12), theme::SURFACE, "{r:?}");
        }
        // The rendered frame is not blank: some pixel differs from the ground.
        let bytes = c.xrgb_bytes();
        assert!(bytes.chunks(4).any(|q| {
            let v = u32::from_le_bytes([q[0], q[1], q[2], q[3]]) & 0xff_ffff;
            v != theme::BG && v != theme::BG_DARK
        }));
    }

    #[test]
    fn landscape_launcher_uses_the_full_logical_surface() {
        let ty = Type::load().expect("embedded fonts load");
        let canvas = compose(&profile(), 9 * 60 + 41, &ty, 2340, 1080, &state());
        assert_eq!((canvas.width(), canvas.height()), (2340, 1080));
        assert_eq!(canvas.get(1170, 10), theme::BG_DARK);
        assert_ne!(
            canvas.get(1150, 180),
            theme::BG,
            "right-side app grid must be visible"
        );
        for r in tile_rects((2340, 1080), state().tiles.len()) {
            assert_eq!(canvas.get(r.x + r.w / 2, r.y + 8), theme::SURFACE, "{r:?}");
        }
    }

    #[test]
    fn every_interactive_page_renders() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let base = state();
        let reference_pages = (0..base.tiles.len())
            .filter(|i| !base.tiles[*i].launchable)
            .map(Page::Reference);
        for page in reference_pages.chain([Page::QuickSettings, Page::Notifications]) {
            let mut state = base.clone();
            state.page = page;
            let c = compose(&p, 600, &ty, 1080, 2340, &state);
            assert_eq!((c.width(), c.height()), (1080, 2340));
            assert_ne!(c.get(300, 500), theme::BG, "{page:?}");
        }
        // An unknown or out-of-range reference index renders the generic page.
        let mut state = base.clone();
        state.page = Page::Reference(99);
        let c = compose(&p, 600, &ty, 1080, 2340, &state);
        assert_ne!(c.get(300, 500), theme::BG);
    }

    #[test]
    fn running_starting_and_crashed_states_are_visible() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let mut state = state();
        let plain = compose(&p, 600, &ty, 1080, 2340, &state);
        let r = tile_rects((1080, 2340), 7)[0];
        let dot = (r.x + r.w - scale(34, 1080), r.y + scale(34, 1080));
        assert_eq!(plain.get(dot.0, dot.1), theme::SURFACE);

        state.tiles[0].running = true;
        let running = compose(&p, 600, &ty, 1080, 2340, &state);
        assert_eq!(
            running.get(dot.0, dot.1),
            tone_color(state.tiles[0].icon.tone),
            "running dot in the tile's accent"
        );

        state.tiles[0].starting = true;
        let starting = compose(&p, 600, &ty, 1080, 2340, &state);
        let label_y = r.y + r.h - scale(44, 1080) - 10;
        let label_differs =
            (r.x..r.x + r.w).any(|x| starting.get(x, label_y) != running.get(x, label_y));
        assert!(label_differs, "starting label replaces the name");

        state.crash_notice = Some(CrashNotice {
            app: state.tiles[0].id.clone(),
            name: state.tiles[0].name.clone(),
            reason: "signal 11".to_owned(),
        });
        let crashed = compose(&p, 600, &ty, 1080, 2340, &state);
        let banner_y = scale(944, 1080) + scale(56, 1080);
        assert_eq!(crashed.get(540, banner_y - scale(40, 1080)), theme::SURFACE);
        assert_eq!(
            crashed.get(scale(48, 1080) + scale(22, 1080), banner_y),
            theme::RED,
            "red accent bar"
        );
        assert_eq!(
            plain.get(540, banner_y),
            theme::BG,
            "no banner without a crash"
        );
    }

    #[test]
    fn glyph_falls_back_to_the_first_letter_when_the_face_lacks_it() {
        let ty = Type::load().expect("embedded fonts load");
        let mut tile = test_tiles().remove(0);
        tile.icon.glyph = ">_".to_owned();
        assert_eq!(tile_glyph(&ty, &tile), ">_");
        tile.icon.glyph = "\u{1f4de}".to_owned();
        tile.name = "phone".to_owned();
        assert_eq!(tile_glyph(&ty, &tile), "P");
        tile.icon.glyph = String::new();
        assert_eq!(tile_glyph(&ty, &tile), "P");
    }

    #[test]
    fn status_bar_height_stays_put_when_the_shade_opens() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        // Include the Cocoa QEMU panel (830) where the old unit scale truncated to 1
        // and jumped the bar from 55px on the launcher to 72px on notifications.
        for width in [830_u32, 1080, 1200] {
            let height = width * 2340 / 1080;
            let launcher = compose(&p, 600, &ty, width, height, &state());
            let mut shade_state = state();
            shade_state.page = Page::Notifications;
            let shade = compose(&p, 600, &ty, width, height, &shade_state);
            let mut quick_state = state();
            quick_state.page = Page::QuickSettings;
            let quick = compose(&p, 600, &ty, width, height, &quick_state);

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
