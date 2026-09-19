//! Composition of the basic screen. Omarchy look and feel (Tokyo Night palette,
//! monospace type, flat bordered surfaces); GrapheneOS-style status bar,
//! minimal launcher grid, and bottom gesture pill where mobile UX is needed.
//! This is a QEMU reference surface rendered from the mock profile, not the
//! product shell.

use crate::draw::{Canvas, Rect, Style, Type};
use crate::profile::Profile;
use crate::theme;

pub fn compose(p: &Profile, minute_of_day: u32, ty: &Type, width: u32, height: u32) -> Canvas {
    let w = width as i32;
    let h = height as i32;
    // All fixed values are tuned for the profile's 1080x2340 portrait panel and
    // scaled linearly from the actual width so other portrait sizes stay usable.
    let s = |v: i32| v * w / 1080;
    let mut c = Canvas::new(w, h, theme::BG);
    let hour = minute_of_day / 60;
    let clock = format!("{:02}:{:02}", hour, minute_of_day % 60);

    status_bar(&mut c, p, ty, &clock, s(72), s(40));

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
        s(1).max(1),
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
        s(1).max(1),
    );

    launcher_grid(&mut c, ty, margin, gap, s(1080), s(1).max(1));

    // Honest footer label and GrapheneOS-style gesture pill.
    let caption = "QEMU reference surface - Rust basic screen";
    let cap_style = Style::regular(s(26) as f32, theme::FG_DIM);
    let capw = ty.width(cap_style, caption);
    ty.text(&mut c, cap_style, caption, (w - capw) / 2, h - s(150));
    c.fill_rounded(
        Rect {
            x: (w - s(320)) / 2,
            y: h - s(60),
            w: s(320),
            h: s(12),
        },
        s(6),
        theme::FG_DIM,
    );
    c
}

fn status_bar(c: &mut Canvas, p: &Profile, ty: &Type, clock: &str, bar_h: i32, pad: i32) {
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

fn card(c: &mut Canvas, ty: &Type, r: Rect, title: &str, s1: i32) {
    c.fill_rounded(r, 24 * s1, theme::SURFACE);
    c.stroke_rounded(r, 24 * s1, 2, theme::BORDER);
    ty.text(
        c,
        Style::regular((28 * s1) as f32, theme::FG_DIM),
        title,
        r.x + 32 * s1,
        r.y + 62 * s1,
    );
}

fn battery_card(c: &mut Canvas, p: &Profile, ty: &Type, r: Rect, s1: i32) {
    card(c, ty, r, "BATTERY", s1);
    let color = if p.charging {
        theme::GREEN
    } else if p.battery_percent < 20 {
        theme::RED
    } else {
        theme::BLUE
    };
    ty.text(
        c,
        Style::bold((72 * s1) as f32, color),
        &format!("{}%", p.battery_percent),
        r.x + 32 * s1,
        r.y + 160 * s1,
    );
    let track = Rect {
        x: r.x + 32 * s1,
        y: r.y + 190 * s1,
        w: r.w - 64 * s1,
        h: 14 * s1,
    };
    c.fill_rounded(track, 7 * s1, theme::SURFACE_HI);
    let fill_w = track.w * i32::from(p.battery_percent.min(100)) / 100;
    if fill_w > 0 {
        c.fill_rounded(
            Rect {
                x: track.x,
                y: track.y,
                w: fill_w,
                h: track.h,
            },
            7 * s1,
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
        Style::regular((26 * s1) as f32, theme::FG_DIM),
        status,
        r.x + 32 * s1,
        r.y + 258 * s1,
    );
}

fn network_card(c: &mut Canvas, p: &Profile, ty: &Type, r: Rect, s1: i32) {
    card(c, ty, r, "NETWORK", s1);
    let connected = p.wifi == "connected";
    ty.text(
        c,
        Style::bold(
            (48 * s1) as f32,
            if connected {
                theme::CYAN
            } else {
                theme::FG_DIM
            },
        ),
        "Wi-Fi",
        r.x + 32 * s1,
        r.y + 140 * s1,
    );
    let wifi_line = if connected {
        "Connected"
    } else {
        p.wifi.as_str()
    };
    ty.text(
        c,
        Style::regular((30 * s1) as f32, theme::FG),
        wifi_line,
        r.x + 32 * s1,
        r.y + 196 * s1,
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
        Style::regular((26 * s1) as f32, theme::FG_DIM),
        &cell,
        r.x + 32 * s1,
        r.y + 258 * s1,
    );
}

fn launcher_grid(c: &mut Canvas, ty: &Type, margin: i32, gap: i32, top: i32, s1: i32) {
    let apps: [(&str, u32); 6] = [
        ("Settings", theme::BLUE),
        ("Messages", theme::GREEN),
        ("Terminal", theme::PURPLE),
        ("Dialer", theme::CYAN),
        ("Camera", theme::YELLOW),
        ("Files", theme::ORANGE),
    ];
    let tile = (c.width() - 2 * margin - 2 * gap) / 3;
    for (i, (label, accent)) in apps.into_iter().enumerate() {
        let col = (i % 3) as i32;
        let row = (i / 3) as i32;
        let r = Rect {
            x: margin + col * (tile + gap),
            y: top + row * (tile + gap),
            w: tile,
            h: tile,
        };
        c.fill_rounded(r, 28 * s1, theme::SURFACE);
        c.stroke_rounded(r, 28 * s1, 2, theme::BORDER);
        let icx = r.x + r.w / 2;
        let icy = r.y + 118 * s1;
        app_icon(c, ty, label, accent, icx, icy, s1);
        let label_style = Style::regular((30 * s1) as f32, theme::FG);
        let lw = ty.width(label_style, label);
        ty.text(c, label_style, label, icx - lw / 2, r.y + r.h - 44 * s1);
    }
}

fn app_icon(c: &mut Canvas, ty: &Type, label: &str, accent: u32, cx: i32, cy: i32, s1: i32) {
    let u = |v: i32| v * s1;
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
                Style::bold((36 * s1) as f32, accent),
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
            modem: "registered".into(),
            sim: "present".into(),
        }
    }

    #[test]
    fn composes_non_blank_portrait_surface() {
        let ty = Type::load().expect("embedded fonts load");
        let p = profile();
        let c = compose(&p, 9 * 60 + 41, &ty, p.display_width, p.display_height);
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
}
