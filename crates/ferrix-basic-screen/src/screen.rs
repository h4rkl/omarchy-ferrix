//! Composition of the basic screen in the Omarchy Ferrix visual system
//! (`docs/design.md`): dark slate surfaces, steel type, one rust accent per
//! region, hairline borders, tight radii, JetBrains Mono. GrapheneOS-style
//! status bar, minimal launcher grid, and bottom gesture pill where mobile UX
//! is needed. Colors come from `ferrix_ui::theme::get()` so a loaded
//! `colors.toml` restyles every surface. This is a reference surface
//! rendered from the connected target's state, not the product shell.

use ferrix_hardware_api::{Capability, CapabilityMatrix, CapabilityStatus, HardwareState};
use ferrix_ui::draw::{Canvas, Rect, Style, Type, Weight};
use ferrix_ui::theme;

use crate::catalog::{Tile, tone_color};
use crate::interaction::{Page, Target, UiState, tile_rects};
use crate::status::Reach;

/// Footer caption of the launcher. Says what this surface is, not what it
/// wants to be.
const CAPTION: &str = "reference surface · not the product shell";

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

/// A 1-2 px hairline that never truncates to nothing on narrow panels.
fn hairline(basis: i32) -> i32 {
    scale(2, basis).max(1)
}

/// The identity line under the clock: product, target, and where the state
/// comes from. `target` is the hardware target id the session connected to;
/// the source word summarises its capability matrix ([`matrix_word`]).
pub fn identity_line(target: &str, capabilities: &CapabilityMatrix) -> String {
    format!(
        "ferrix \u{b7} {target} \u{b7} {}",
        matrix_word(capabilities)
    )
}

/// `text` at `size`, shrunk towards `min` until it fits `max_width`, and
/// ellipsized only if it still does not fit at `min`.
fn fit_line(
    ty: &Type,
    text: &str,
    size: i32,
    min: i32,
    max_width: i32,
    color: u32,
) -> (Style, String) {
    let mut px = size;
    while px > min && ty.width(Style::regular(px as f32, color), text) > max_width {
        px -= 1;
    }
    let style = Style::regular(px as f32, color);
    (
        style,
        ferrix_ui::widgets::ellipsize(ty, style, text, max_width),
    )
}

/// The user-facing word for one capability's status. `Unsupported` reads as
/// "unadmitted": a value may still be shown, but the target's checked-in
/// matrix does not claim the capability.
pub fn capability_word(status: CapabilityStatus) -> &'static str {
    match status {
        CapabilityStatus::Supported => "supported",
        CapabilityStatus::Mocked => "mock",
        CapabilityStatus::Unsupported => "unadmitted",
    }
}

/// One word for a whole matrix: `mock` when any capability is mocked, else
/// `supported` only when every target capability is, else `unadmitted`.
/// `MockControl` is a test seam, not a target capability, so it is ignored.
pub fn matrix_word(capabilities: &CapabilityMatrix) -> &'static str {
    let statuses = || {
        Capability::ALL
            .iter()
            .filter(|c| **c != Capability::MockControl)
            .map(|c| capabilities.status(*c))
    };
    if statuses().any(|s| s == CapabilityStatus::Mocked) {
        capability_word(CapabilityStatus::Mocked)
    } else if statuses().all(|s| s == CapabilityStatus::Supported) {
        capability_word(CapabilityStatus::Supported)
    } else {
        capability_word(CapabilityStatus::Unsupported)
    }
}

pub fn compose(
    p: &HardwareState,
    minute_of_day: u32,
    ty: &Type,
    width: u32,
    height: u32,
    state: &UiState,
) -> Canvas {
    let t = theme::get();
    let w = width as i32;
    let h = height as i32;
    let basis = w.min(h);
    let s = |v: i32| scale(v, basis);
    let mut c = Canvas::new(w, h, t.bg);
    let clock = ferrix_ui::clock::hhmm(minute_of_day);

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

    // Large clock and identity line, GrapheneOS lock-screen placement.
    let clock_style = Style::bold(s(170) as f32, t.fg);
    let cw = ty.width(clock_style, &clock);
    ty.text(&mut c, clock_style, &clock, (w - cw) / 2, s(430));
    let (ident_style, identity) =
        fit_line(ty, &state.identity, s(34), s(24), w - 2 * s(48), t.fg_dim);
    let iw = ty.width(ident_style, &identity);
    ty.text(&mut c, ident_style, &identity, (w - iw) / 2, s(520));

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
        capability_word(state.capabilities.status(Capability::Power)),
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
        state,
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
            &notice.text(),
        );
    }
    launcher_grid(&mut c, ty, state);

    // Honest footer label and GrapheneOS-style gesture pill.
    let cap_style = Style::regular(s(26) as f32, t.fg_dim);
    let capw = ty.width(cap_style, CAPTION);
    ty.text(&mut c, cap_style, CAPTION, (w - capw) / 2, h - s(150));
    gesture_pill(&mut c, state);
    c
}

fn launcher_landscape(c: &mut Canvas, p: &HardwareState, ty: &Type, clock: &str, state: &UiState) {
    let t = theme::get();
    let basis = design_width(c);
    let s = |v: i32| scale(v, basis);
    status_bar(c, p, ty, clock, s(72), s(40), state);

    let clock_style = Style::bold(s(150) as f32, t.fg);
    let clock_width = ty.width(clock_style, clock);
    ty.text(c, clock_style, clock, s(460) - clock_width / 2, s(280));
    let ident_style = Style::regular(s(32) as f32, t.fg_dim);
    let ident_width = ty.width(ident_style, &state.identity);
    ty.text(
        c,
        ident_style,
        &state.identity,
        s(460) - ident_width / 2,
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
        capability_word(state.capabilities.status(Capability::Power)),
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
        state,
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
            &notice.text(),
        );
    }
    launcher_grid(c, ty, state);
    gesture_pill(c, state);
}

/// The compositor's home indicator: the one pill shape in the system.
fn gesture_pill(c: &mut Canvas, state: &UiState) {
    let t = theme::get();
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
            t.rust
        } else {
            t.fg_dim
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
    let t = theme::get();
    let w = c.width();
    c.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w,
            h: bar_h,
        },
        if state.pressed == Some(Target::Status) {
            t.surface_hi
        } else {
            t.bg_dark
        },
    );
    let px = (bar_h * 5 / 12) as f32;
    let baseline = bar_h * 2 / 3;
    // Clock on the left, GrapheneOS style.
    ty.text(c, Style::bold(px, t.fg), clock, pad, baseline);

    // Right-aligned: signal bars, wifi arcs, battery percent and gauge.
    let mid = bar_h / 2;
    let pct = format!("{}%", p.power.battery_percent);
    let batt_color = if p.power.charging {
        t.ok
    } else if p.power.battery_percent < 20 {
        t.err
    } else {
        t.fg
    };
    let mut x = w - pad;
    x -= battery_icon(c, x, mid, p, batt_color);
    let pct_style = Style::regular(px, t.fg);
    x -= ty.width(pct_style, &pct) + bar_h / 4;
    ty.text(c, pct_style, &pct, x, baseline);
    x -= bar_h / 3;
    x -= wifi_icon(c, x, mid + bar_h / 4, bar_h / 2, state.net.reach);
    x -= bar_h / 3;
    signal_bars(c, x, mid + bar_h / 4, bar_h / 2, state.tele.bars);
}

/// Draws a battery outline with proportional fill, right edge at `right`.
/// Returns the consumed width.
fn battery_icon(c: &mut Canvas, right: i32, mid: i32, p: &HardwareState, color: u32) -> i32 {
    let t = theme::get();
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
        3,
        3,
        t.fg_dim,
    );
    c.fill_rounded(
        Rect {
            x: x + bw - 4,
            y: mid - 5,
            w: 4,
            h: 10,
        },
        1,
        t.fg_dim,
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
            1,
            color,
        );
    }
    if p.power.charging {
        bolt(c, x + (bw - 4) / 2, mid, bh - 8, t.bg_dark);
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

/// Network glyphs are steel: the secondary accent, never rust. The lit arcs
/// follow the connectivity check (`local`/`portal` one, `limited` two,
/// `full` three); an unlinked radio keeps the dot only.
fn wifi_icon(c: &mut Canvas, right: i32, base: i32, size: i32, reach: Reach) -> i32 {
    let t = theme::get();
    let cx = right - size;
    let lit = reach.arcs() as usize;
    let color = if lit > 0 { t.steel } else { t.fg_dim };
    for (i, r) in [size / 4, size * 5 / 8, size].into_iter().enumerate() {
        let on = i < lit;
        for dy in -r - 2..=0 {
            for dx in -r - 2..=r + 2 {
                let d2 = dx * dx + dy * dy;
                if d2 >= (r - 2) * (r - 2) && d2 <= (r + 2) * (r + 2) && dx.abs() <= -dy {
                    c.put(cx + dx, base + dy, if on { color } else { t.surface_hi });
                }
            }
        }
    }
    c.fill_circle(cx, base, 4, color);
    2 * size
}

/// `bars` (0..=4) lit of four; the daemon's signal when reachable.
fn signal_bars(c: &mut Canvas, right: i32, base: i32, size: i32, bars: u8) {
    let t = theme::get();
    let bw = size / 4;
    let gap = 4;
    for i in 0..4 {
        let bh = size * (i + 1) / 4 + 4;
        let on = i < i32::from(bars.min(4));
        c.fill_rect(
            Rect {
                x: right - (4 - i) * (bw + gap),
                y: base - bh,
                w: bw,
                h: bh,
            },
            if on { t.steel } else { t.surface_hi },
        );
    }
}

/// A card: slate surface, hairline border, tight radius. Pressed raises the
/// fill and outlines it in rust.
fn card(c: &mut Canvas, ty: &Type, r: Rect, title: &str, pressed: bool) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    c.fill_rounded(r, s(10), if pressed { t.surface_hi } else { t.surface });
    c.stroke_rounded(
        r,
        s(10),
        if pressed { s(3) } else { hairline(width) },
        if pressed { t.rust } else { t.border },
    );
    ty.text(
        c,
        Style::bold(s(26) as f32, t.fg_dim),
        title,
        r.x + s(32),
        r.y + s(62),
    );
}

fn battery_card(
    c: &mut Canvas,
    p: &HardwareState,
    ty: &Type,
    r: Rect,
    pressed: bool,
    source: &str,
) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    card(c, ty, r, "BATTERY", pressed);
    let color = if p.power.charging {
        t.ok
    } else if p.power.battery_percent < 20 {
        t.err
    } else {
        t.fg
    };
    let pct = format!("{}%", p.power.battery_percent);
    let pct_style = Style::bold(s(72) as f32, color);
    ty.text(c, pct_style, &pct, r.x + s(32), r.y + s(160));
    let track = Rect {
        x: r.x + s(32),
        y: r.y + s(190),
        w: r.w - s(64),
        h: s(14),
    };
    c.fill_rounded(track, s(4), t.surface_hi);
    let fill_w = track.w * i32::from(p.power.battery_percent.min(100)) / 100;
    if fill_w > 0 {
        c.fill_rounded(
            Rect {
                x: track.x,
                y: track.y,
                w: fill_w,
                h: track.h,
            },
            s(4),
            color,
        );
    }
    if p.power.charging {
        let pct_w = ty.width(pct_style, &pct);
        bolt(
            c,
            r.x + s(32) + pct_w + s(36),
            r.y + s(160) - s(26),
            s(52),
            color,
        );
    }
    let status = format!(
        "{} \u{b7} {source}",
        if p.power.charging {
            "charging"
        } else {
            "discharging"
        }
    );
    ty.text(
        c,
        Style::regular(s(26) as f32, t.fg_dim),
        &status,
        r.x + s(32),
        r.y + s(258),
    );
}

/// The network card reads the status summaries (service when reachable,
/// hardware-api otherwise) and names the source on its last line.
fn network_card(
    c: &mut Canvas,
    _p: &HardwareState,
    ty: &Type,
    r: Rect,
    pressed: bool,
    state: &UiState,
) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    card(c, ty, r, "NETWORK", pressed);
    let connected = state.net.connected;
    ty.text(
        c,
        Style::bold(s(48) as f32, if connected { t.steel } else { t.fg_dim }),
        "Wi-Fi",
        r.x + s(32),
        r.y + s(140),
    );
    let wifi_line = if connected {
        format!("connected \u{b7} {}", state.net.reach.name())
    } else if state.net.wifi_on {
        "disconnected".to_owned()
    } else {
        "disabled".to_owned()
    };
    ty.text(
        c,
        Style::regular(s(30) as f32, t.fg),
        &wifi_line,
        r.x + s(32),
        r.y + s(196),
    );
    let cell = format!(
        "cell {} \u{b7} sim {} \u{b7} {}",
        if state.tele.registered {
            "ok"
        } else {
            state.tele.registration.as_str()
        },
        state.tele.sim,
        state.tele.source.name()
    );
    let cell_style = Style::regular(s(26) as f32, t.fg_dim);
    let cell = ferrix_ui::widgets::ellipsize(ty, cell_style, &cell, r.w - s(64));
    ty.text(c, cell_style, &cell, r.x + s(32), r.y + s(258));
}

/// The launcher grid: three columns, rows growing with the catalog. Tiles
/// share their rectangles with the hit test. The Terminal gets a quiet
/// `rust_dim` identity rule while its glyph carries the sharp rust signature;
/// other glyphs take their descriptor tone. The pressed outline and running
/// dot are rust. A launched app shows the dot while it runs and a
/// dimmed "starting" label until `ferrix-appd` reports it in the foreground
/// or background.
fn launcher_grid(c: &mut Canvas, ty: &Type, state: &UiState) {
    let t = theme::get();
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
        // Terminal is Ferrix's terminal-first anchor: its deep rust panel is
        // visibly warmer than the utility tiles, but `rust_dim` keeps a whole
        // tile comfortable rather than using an alert-bright orange fill.
        let terminal = tile.kind() == "terminal";
        let fill = if terminal {
            t.rust_dim
        } else if pressed {
            t.surface_hi
        } else {
            t.surface
        };
        c.fill_rounded(r, s(10), fill);
        c.stroke_rounded(
            r,
            s(10),
            if pressed { s(3) } else { hairline(width) },
            if pressed { t.rust } else { t.border },
        );
        // A low-luminance oxidized-orange rule gives the Terminal a durable
        // place in the launcher without turning the whole grid into a bright
        // alert surface. It is decorative only; the tile rect stays the hit
        // target.
        if tile.kind() == "terminal" {
            c.fill_rounded(
                Rect {
                    x: r.x + s(10),
                    y: r.y + s(10),
                    w: r.w - s(20),
                    h: hairline(width),
                },
                hairline(width),
                t.rust_hi,
            );
        }
        let icx = r.x + r.w / 2;
        let icy = r.y + s(if landscape { 105 } else { 118 });
        app_icon(c, ty, tile, accent, icx, icy);
        let (label, color) = if tile.starting {
            ("starting".to_owned(), t.fg_dim)
        } else {
            (tile.name.clone(), t.fg)
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
            c.fill_circle(r.x + r.w - s(34), r.y + s(34), s(9), t.rust);
        }
    }
}

/// Tile artwork. The reference entries keep their drawn icons; any other
/// app shows its descriptor glyph when the face has it, else the first
/// letter of its name (never a tofu box).
fn app_icon(c: &mut Canvas, ty: &Type, tile: &Tile, accent: u32, cx: i32, cy: i32) {
    let t = theme::get();
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
                    u(3),
                    t.surface_hi,
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
                u(8),
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
                    u(2),
                    t.bg,
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
                u(6),
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
        "phone" => {
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
                u(8),
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
                u(3),
                accent,
            );
            c.fill_rounded(
                Rect {
                    x: cx - u(44),
                    y: cy - u(26),
                    w: u(88),
                    h: u(58),
                },
                u(5),
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

/// Restrained crash notice: a flat surface with an `err` rule at the left
/// and one line of text. Dismissed by the next tap or by relaunching the app.
fn crash_banner(c: &mut Canvas, ty: &Type, r: Rect, text: &str) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    c.fill_rounded(r, s(10), t.surface);
    c.stroke_rounded(r, s(10), hairline(width), t.border);
    c.fill_rounded(
        Rect {
            x: r.x + s(18),
            y: r.y + s(22),
            w: s(8),
            h: r.h - s(44),
        },
        s(2),
        t.err,
    );
    let mut style = Style::regular(s(28) as f32, t.fg);
    let max = r.w - s(72);
    while ty.width(style, text) > max && style.px > 12.0 {
        style.px -= 2.0;
    }
    let baseline = r.y + r.h / 2 + (style.px * 0.36) as i32;
    ty.text(c, style, text, r.x + s(48), baseline);
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

/// Back chevron, title, and a warm oxidized-orange rule: section emphasis is
/// thin and low-luminance, never a filled header.
fn page_header(c: &mut Canvas, ty: &Type, title: &str, state: &UiState) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let back = Rect {
        x: s(36),
        y: s(106),
        w: s(160),
        h: s(104),
    };
    if state.pressed == Some(Target::Back) {
        c.fill_rounded(back, s(6), t.surface_hi);
    }
    ty.text(c, Style::bold(s(48) as f32, t.fg_dim), "<", s(64), s(178));
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

/// Illustrative content of a non-launchable catalog entry, keyed by the id's
/// last segment. Unknown entries get a generic "not installed" page. Every
/// line names the thing, then its state, and says where the state comes from.
fn reference_page(c: &mut Canvas, ty: &Type, tile: Option<&Tile>, state: &UiState) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    let name = tile.map_or("Unknown", |t| t.name.as_str());
    page_header(c, ty, name, state);
    let fallback_title = format!("{name} \u{b7} not installed");
    // Phone, Messages, Contacts, and Settings are real applications launched
    // by `ferrix-appd`; only camera and files keep illustrative pages. The
    // hardware API defines neither a camera nor a storage capability, so the
    // pages say that about whichever target is connected.
    let (accent, eyebrow, title, detail) = match tile.map_or("", Tile::kind) {
        "camera" => (
            t.fg_dim,
            "CAMERA",
            "No camera capability",
            "The hardware API defines no camera capability on any target yet.",
        ),
        "files" => (
            t.fg_dim,
            "FILES",
            "No file browser",
            "The hardware API defines no storage capability on any target yet.",
        ),
        _ => (
            tile.map_or(t.fg_dim, |t| tone_color(t.icon.tone)),
            "APP",
            fallback_title.as_str(),
            "No runtime provides this application. The catalog entry is a placeholder.",
        ),
    };
    let panel = Rect {
        x: s(72),
        y: s(360),
        w: c.width() - s(144),
        h: s(560),
    };
    c.fill_rounded(panel, s(10), t.surface);
    c.stroke_rounded(panel, s(10), hairline(width), t.border);
    ty.text(
        c,
        Style::bold(s(28) as f32, accent),
        eyebrow,
        s(120),
        s(450),
    );
    ty.text(c, Style::bold(s(58) as f32, t.fg), title, s(120), s(570));
    wrap_text(c, ty, detail, s(120), s(675), c.width() - s(240), s(34));
    ty.text(
        c,
        Style::regular(s(26) as f32, t.fg_dim),
        "left edge \u{b7} back",
        s(120),
        s(850),
    );
}

fn quick_settings(c: &mut Canvas, ty: &Type, state: &UiState) {
    let t = theme::get();
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
    );
    ty.text(
        c,
        Style::bold(s(28) as f32, t.fg_dim),
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
    c.fill_rounded(track, s(4), t.surface_hi);
    let fill = track.w * i32::from(state.brightness) / 100;
    c.fill_rounded(Rect { w: fill, ..track }, s(4), t.rust);
    c.fill_circle(track.x + fill, track.y + track.h / 2, s(34), t.fg);
    let value = format!("{}%", state.brightness);
    ty.text(c, Style::bold(s(34) as f32, t.fg), &value, s(96), s(1210));
    // Long physical target ids wrap instead of running off narrow panels.
    wrap_text(
        c,
        ty,
        &brightness_caption(state),
        s(96),
        s(1280),
        c.width() - s(192),
        s(27),
    );
}

/// Where the brightness value comes from: the display capability's status on
/// the connected target, e.g. `display brightness · mock · qemu-aarch64`.
fn brightness_caption(state: &UiState) -> String {
    format!(
        "display brightness \u{b7} {} \u{b7} {}",
        capability_word(state.capabilities.status(Capability::Display)),
        state.target
    )
}

/// The session notification's body: the connected target, then input help.
fn session_ready_detail(state: &UiState) -> String {
    format!(
        "{} \u{b7} touch input active. Bottom-edge swipe returns home.",
        state.target
    )
}

/// A quick-settings toggle. Off is a plain card; on keeps the slate fill and
/// takes a rust outline and a rust state word, so the accent marks exactly
/// the thing that is active.
fn toggle(c: &mut Canvas, ty: &Type, r: Rect, label: &str, on: bool, pressed: bool) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    c.fill_rounded(r, s(10), if pressed { t.surface_hi } else { t.surface });
    let (thickness, color) = match (on, pressed) {
        (_, true) => (s(3), t.rust_hi),
        (true, false) => (s(3), t.rust),
        (false, false) => (hairline(width), t.border),
    };
    c.stroke_rounded(r, s(10), thickness, color);
    ty.text(
        c,
        Style::bold(s(44) as f32, t.fg),
        label,
        r.x + s(38),
        r.y + s(104),
    );
    ty.text(
        c,
        Style::bold(s(30) as f32, if on { t.rust } else { t.fg_dim }),
        if on { "on" } else { "off" },
        r.x + s(38),
        r.y + s(190),
    );
}

fn notifications(c: &mut Canvas, ty: &Type, state: &UiState) {
    let t = theme::get();
    let width = design_width(c);
    let s = |v: i32| scale(v, width);
    page_header(c, ty, "Notifications", state);
    if !state.notification_visible {
        let title = "No notifications";
        let style = Style::bold(s(48) as f32, t.fg);
        ty.text(
            c,
            style,
            title,
            (c.width() - ty.width(style, title)) / 2,
            s(780),
        );
        let detail = "bottom edge \u{b7} home";
        let detail_style = Style::regular(s(28) as f32, t.fg_dim);
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
    c.fill_rounded(card, s(10), t.surface);
    c.stroke_rounded(card, s(10), hairline(width), t.border);
    // The rust rule at the left is the region's one accent.
    c.fill_rounded(
        Rect {
            x: card.x + s(18),
            y: card.y + s(28),
            w: s(8),
            h: card.h - s(56),
        },
        s(2),
        t.rust,
    );
    ty.text(
        c,
        Style::bold(s(28) as f32, t.fg_dim),
        "SYSTEM",
        s(116),
        s(475),
    );
    ty.text(
        c,
        Style::bold(s(48) as f32, t.fg),
        "Session ready",
        s(116),
        s(575),
    );
    wrap_text(
        c,
        ty,
        &session_ready_detail(state),
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
    let pressed = state.pressed == Some(Target::ClearNotifications);
    c.fill_rounded(clear, s(10), if pressed { t.surface_hi } else { t.surface });
    c.stroke_rounded(
        clear,
        s(10),
        if pressed { s(3) } else { hairline(width) },
        if pressed { t.rust } else { t.border },
    );
    let label = "Clear";
    let style = Style::bold(s(38) as f32, t.fg);
    ty.text(
        c,
        style,
        label,
        (c.width() - ty.width(style, label)) / 2,
        s(1790),
    );
}

fn wrap_text(c: &mut Canvas, ty: &Type, text: &str, x: i32, y: i32, max_width: i32, px: i32) {
    let style = Style::regular(px as f32, theme::get().fg_dim);
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
        let mut state = UiState::new(true, true, test_tiles());
        let hw = profile();
        state.tele = crate::status::telephony_summary(None, &hw);
        state.net = crate::status::network_summary(None, &hw);
        state
    }

    fn terminal_index(state: &UiState) -> usize {
        state
            .tiles
            .iter()
            .position(|t| t.kind() == "terminal")
            .expect("terminal tile")
    }

    #[test]
    fn identity_line_names_product_target_and_state_source() {
        let mocked = CapabilityMatrix::uniform(CapabilityStatus::Mocked);
        assert_eq!(
            identity_line("qemu-aarch64", &mocked),
            "ferrix \u{b7} qemu-aarch64 \u{b7} mock"
        );
        assert_eq!(state().identity, identity_line("qemu-aarch64", &mocked));
        let unadmitted = CapabilityMatrix::uniform(CapabilityStatus::Unsupported);
        assert_eq!(
            identity_line("google-pixel-9a-tegu-gxq96-mp1-0", &unadmitted),
            "ferrix \u{b7} google-pixel-9a-tegu-gxq96-mp1-0 \u{b7} unadmitted"
        );
        let supported = CapabilityMatrix::uniform(CapabilityStatus::Supported)
            .with(Capability::MockControl, CapabilityStatus::Unsupported);
        assert_eq!(matrix_word(&supported), "supported");
        assert_eq!(
            matrix_word(&supported.with(Capability::Audio, CapabilityStatus::Unsupported)),
            "unadmitted"
        );
    }

    #[test]
    fn session_copy_follows_the_connected_target_not_a_fixed_label() {
        let mut ui = state();
        assert_eq!(
            brightness_caption(&ui),
            "display brightness \u{b7} mock \u{b7} qemu-aarch64"
        );
        assert!(session_ready_detail(&ui).starts_with("qemu-aarch64 \u{b7} "));
        ui.set_session(
            "google-pixel-9a-tegu-gxq96-mp1-0",
            CapabilityMatrix::uniform(CapabilityStatus::Unsupported)
                .with(Capability::Display, CapabilityStatus::Supported),
        );
        assert_eq!(
            brightness_caption(&ui),
            "display brightness \u{b7} supported \u{b7} google-pixel-9a-tegu-gxq96-mp1-0"
        );
        assert!(session_ready_detail(&ui).starts_with("google-pixel-9a-tegu-gxq96-mp1-0"));
        assert!(!session_ready_detail(&ui).contains("QEMU"));
        assert!(!brightness_caption(&ui).contains("QEMU"));
    }

    #[test]
    fn charging_changes_the_battery_indicator() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let mut p = profile();
        let state = state();
        let before = compose(&p, 600, &ty, 1080, 2340, &state);
        p.power.charging = true;
        let after = compose(&p, 600, &ty, 1080, 2340, &state);
        // The status-bar battery fill turns `ok` and gains a bolt; the card
        // headline turns `ok` too. Compare the card headline region.
        let card_y = scale(620, 1080) + scale(160, 1080) - scale(40, 1080);
        let card_x = scale(48, 1080) + scale(32, 1080) + scale(30, 1080);
        let mut differs = false;
        let mut ok_seen = false;
        for dy in 0..scale(60, 1080) {
            for dx in 0..scale(200, 1080) {
                let px = after.get(card_x + dx, card_y + dy);
                if before.get(card_x + dx, card_y + dy) != px {
                    differs = true;
                }
                ok_seen |= px == t.ok;
            }
        }
        assert!(differs, "charging must be visible on the battery card");
        assert!(ok_seen, "the charging headline is the `ok` status color");
        let bar_h = scale(72, 1080);
        let bolt_seen = (0..bar_h).any(|dy| {
            (1080 - scale(40, 1080) - 46..1080 - scale(40, 1080))
                .any(|x| after.get(x, dy) == t.bg_dark && before.get(x, dy) != t.bg_dark)
        });
        assert!(
            bolt_seen,
            "status-bar battery must show a bolt cut-out when charging"
        );
    }

    #[test]
    fn composes_non_blank_portrait_surface_with_catalog_tiles() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let state = state();
        assert!(
            state.tiles.len() >= 7,
            "phone, messages, contacts, settings, terminal, camera, files"
        );
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
        assert_eq!(c.get(540, 10), t.bg_dark);
        assert_eq!(c.get(540, 600), t.bg);
        // The terminal anchor is deep rust; the remaining utility tiles are
        // slate. Tile geometry remains shared with the hit test.
        for (tile, r) in state
            .tiles
            .iter()
            .zip(tile_rects((1080, 2340), state.tiles.len()))
        {
            let expected = if tile.kind() == "terminal" {
                t.rust_dim
            } else {
                t.surface
            };
            assert_eq!(c.get(r.x + r.w / 2, r.y + 12), expected, "{r:?}");
        }
        // The rendered frame is not blank: some pixel differs from the ground.
        let bytes = c.xrgb_bytes();
        assert!(bytes.chunks(4).any(|q| {
            let v = u32::from_le_bytes([q[0], q[1], q[2], q[3]]) & 0xff_ffff;
            v != t.bg && v != t.bg_dark
        }));
    }

    #[test]
    fn landscape_launcher_uses_the_full_logical_surface() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let canvas = compose(&profile(), 9 * 60 + 41, &ty, 2340, 1080, &state());
        assert_eq!((canvas.width(), canvas.height()), (2340, 1080));
        assert_eq!(canvas.get(1170, 10), t.bg_dark);
        assert_ne!(
            canvas.get(1150, 180),
            t.bg,
            "right-side app grid must be visible"
        );
        let state = state();
        for (tile, r) in state
            .tiles
            .iter()
            .zip(tile_rects((2340, 1080), state.tiles.len()))
        {
            let expected = if tile.kind() == "terminal" {
                t.rust_dim
            } else {
                t.surface
            };
            assert_eq!(canvas.get(r.x + r.w / 2, r.y + 8), expected, "{r:?}");
        }
    }

    #[test]
    fn every_interactive_page_renders() {
        let t = theme::get();
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
            assert_ne!(c.get(300, 500), t.bg, "{page:?}");
            // The page header's section rule is quiet oxidized orange, not a
            // filled header.
            assert_eq!(c.get(540, scale(230, 1080)), t.rust_dim, "{page:?}");
        }
        // An unknown or out-of-range reference index renders the generic page.
        let mut state = base.clone();
        state.page = Page::Reference(99);
        let c = compose(&p, 600, &ty, 1080, 2340, &state);
        assert_ne!(c.get(300, 500), t.bg);
    }

    #[test]
    fn active_toggles_and_the_brightness_fill_are_rust() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let mut state = state();
        state.page = Page::QuickSettings;
        state.brightness = 50;
        let on = compose(&p, 600, &ty, 1080, 2340, &state);
        // Wi-Fi toggle outline: the top edge row of its rect, mid-width (the
        // stroke is centred on the edge, so the edge row is solid).
        let outline = (scale(86 + 210, 1080), scale(468, 1080));
        assert_eq!(on.get(outline.0, outline.1), t.rust, "active toggle");
        state.wifi_on = false;
        let off = compose(&p, 600, &ty, 1080, 2340, &state);
        assert_eq!(off.get(outline.0, outline.1), t.border, "inactive toggle");
        // Brightness rail: rust to the left of the knob, track to the right.
        let rail_y = scale(1084, 1080) + scale(14, 1080);
        assert_eq!(off.get(scale(130 + 100, 1080), rail_y), t.rust);
        assert_eq!(off.get(scale(130 + 720, 1080), rail_y), t.surface_hi);
    }

    #[test]
    fn running_starting_and_crashed_states_are_visible() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let mut state = state();
        let plain = compose(&p, 600, &ty, 1080, 2340, &state);
        let index = terminal_index(&state);
        let r = tile_rects((1080, 2340), state.tiles.len())[index];
        let dot = (r.x + r.w - scale(34, 1080), r.y + scale(34, 1080));
        assert_eq!(plain.get(dot.0, dot.1), t.rust_dim);

        state.tiles[index].running = true;
        let running = compose(&p, 600, &ty, 1080, 2340, &state);
        assert_eq!(
            running.get(dot.0, dot.1),
            t.rust,
            "running dot is the rust accent"
        );

        state.tiles[index].starting = true;
        let starting = compose(&p, 600, &ty, 1080, 2340, &state);
        let label_y = r.y + r.h - scale(44, 1080) - 10;
        let label_differs =
            (r.x..r.x + r.w).any(|x| starting.get(x, label_y) != running.get(x, label_y));
        assert!(label_differs, "starting label replaces the name");

        state.crash_notice = Some(CrashNotice::crashed(
            state.tiles[index].id.clone(),
            state.tiles[index].name.clone(),
            "signal 11".to_owned(),
        ));
        let crashed = compose(&p, 600, &ty, 1080, 2340, &state);
        let banner_y = scale(944, 1080) + scale(56, 1080);
        assert_eq!(crashed.get(540, banner_y - scale(40, 1080)), t.surface);
        assert_eq!(
            crashed.get(scale(48, 1080) + scale(22, 1080), banner_y),
            t.err,
            "err accent bar"
        );
        assert_eq!(plain.get(540, banner_y), t.bg, "no banner without a crash");
        // A failed launch uses the same banner with the "could not start" line.
        state.crash_notice = Some(CrashNotice::launch_failed(
            state.tiles[index].id.clone(),
            "Browser".to_owned(),
            "no android runtime is available".to_owned(),
        ));
        let failed = compose(&p, 600, &ty, 1080, 2340, &state);
        assert_eq!(
            failed.get(scale(48, 1080) + scale(22, 1080), banner_y),
            t.err
        );
        let text_differs = (scale(120, 1080)..scale(900, 1080))
            .any(|x| failed.get(x, banner_y) != crashed.get(x, banner_y));
        assert!(
            text_differs,
            "launch-failure text differs from the crash text"
        );
    }

    #[test]
    fn pressed_tile_outline_is_rust_whatever_its_tone() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let mut state = state();
        let steel_tile = state
            .tiles
            .iter()
            .position(|tile| tone_color(tile.icon.tone) == t.steel)
            .expect("a steel-toned tile");
        state.pressed = Some(Target::App(steel_tile));
        let c = compose(&p, 600, &ty, 1080, 2340, &state);
        let r = tile_rects((1080, 2340), state.tiles.len())[steel_tile];
        assert_eq!(c.get(r.x + r.w / 2, r.y), t.rust, "pressed outline");
        assert_eq!(c.get(r.x + r.w / 2, r.y + r.h / 2 + 40), t.surface_hi);
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
    fn status_bar_follows_the_service_summaries() {
        let t = theme::get();
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let mut full = state();
        full.tele.bars = 4;
        full.net.reach = Reach::Full;
        let strong = compose(&p, 600, &ty, 1080, 2340, &full);
        let mut weak = state();
        weak.tele.bars = 1;
        weak.net.reach = Reach::Disconnected;
        weak.net.connected = false;
        let faint = compose(&p, 600, &ty, 1080, 2340, &weak);
        let bar_h = scale(72, 1080);
        // Somewhere in the right third of the strip the strong frame shows
        // steel where the weak one shows the dim track.
        let region = (700..1080).flat_map(|x| (0..bar_h).map(move |y| (x, y)));
        let mut steel_to_track = 0;
        for (x, y) in region {
            if strong.get(x, y) == t.steel && faint.get(x, y) == t.surface_hi {
                steel_to_track += 1;
            }
        }
        assert!(
            steel_to_track > 20,
            "signal bars and wifi arcs dim with the summary"
        );
    }

    #[test]
    fn status_bar_height_stays_put_when_the_shade_opens() {
        let t = theme::get();
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
                assert_eq!(frame.get(x, 0), t.bg_dark, "width={width}");
                assert_eq!(frame.get(x, bar_h - 1), t.bg_dark, "width={width}");
                // No separator rule under the bar.
                assert_ne!(frame.get(x, bar_h), t.border, "width={width}");
            }
            // The bar itself is pixel-identical on every page.
            for y in 0..bar_h {
                for col in 0..width as i32 {
                    let pixel = launcher.get(col, y);
                    assert_eq!(shade.get(col, y), pixel, "width={width} ({col},{y})");
                    assert_eq!(quick.get(col, y), pixel, "width={width} ({col},{y})");
                }
            }
        }
    }
}
