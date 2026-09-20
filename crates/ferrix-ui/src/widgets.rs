//! Reusable software-drawn widgets in the Omarchy Ferrix visual system
//! (`docs/design.md`): slate surfaces, hairline borders, 10 px card and 6 px
//! chip radii, one rust accent per region. Geometry helpers return the same
//! rectangles the surfaces hit-test, so interaction tests exercise what the
//! framebuffer shows. Everything is pure drawing over [`Canvas`].

use crate::draw::{Canvas, Rect, Style, Type};
use crate::osk::{self, Key, OskState};
use crate::theme;

/// Scale a 1080-wide design length. Multiply first so narrow panels never
/// truncate to zero.
pub fn scale(v: i32, basis: i32) -> i32 {
    v * basis / 1080
}

/// Stable density basis for both orientations: the shorter side.
pub fn basis(size: (u32, u32)) -> i32 {
    (size.0.min(size.1)) as i32
}

/// Hairline thickness: 2 px at the design width, never less than 1.
pub fn hairline(basis: i32) -> i32 {
    scale(2, basis).max(1)
}

pub fn contains(r: Rect, position: (f64, f64)) -> bool {
    let (x, y) = position;
    x >= f64::from(r.x)
        && x < f64::from(r.x + r.w)
        && y >= f64::from(r.y)
        && y < f64::from(r.y + r.h)
}

pub fn center(r: Rect) -> (f64, f64) {
    (f64::from(r.x + r.w / 2), f64::from(r.y + r.h / 2))
}

/// Height of the top clock strip (72 design px).
pub fn clock_strip_height(basis: i32) -> i32 {
    scale(72, basis)
}

/// The top strip shared with the home surface: `bg_dark`, clock at the left,
/// an optional right-aligned service label in `fg_dim`, and a `rust_dim`
/// rule below.
pub fn clock_strip(c: &mut Canvas, ty: &Type, clock: &str, right_label: Option<&str>) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let bar_h = clock_strip_height(b);
    let pad = scale(40, b);
    c.fill_rect(
        Rect {
            x: 0,
            y: 0,
            w: c.width(),
            h: bar_h,
        },
        t.bg_dark,
    );
    c.fill_rect(
        Rect {
            x: 0,
            y: bar_h,
            w: c.width(),
            h: hairline(b),
        },
        t.rust_dim,
    );
    let px = (bar_h * 5 / 12) as f32;
    ty.text(c, Style::bold(px, t.fg), clock, pad, bar_h * 2 / 3);
    if let Some(label) = right_label {
        let style = Style::regular(px * 0.85, t.fg_dim);
        let w = ty.width(style, label);
        ty.text(c, style, label, c.width() - pad - w, bar_h * 2 / 3);
    }
}

/// The compositor home indicator: the one pill shape in the system.
pub fn gesture_pill(c: &mut Canvas, pressed: bool) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
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

/// Hit rectangle of the header back control.
pub fn back_rect(size: (u32, u32)) -> Rect {
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    Rect {
        x: 0,
        y: s(72),
        w: s(236),
        h: s(180),
    }
}

/// Y coordinate of the header rule; content starts below it.
pub fn header_bottom(basis: i32) -> i32 {
    scale(252, basis)
}

/// Back chevron, title, optional right-aligned status label, and a quiet
/// `rust_dim` rule: never a filled header.
pub fn page_header(
    c: &mut Canvas,
    ty: &Type,
    title: &str,
    status: Option<&str>,
    back_pressed: bool,
) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    if back_pressed {
        c.fill_rounded(
            Rect {
                x: s(36),
                y: s(106),
                w: s(160),
                h: s(104),
            },
            s(8),
            t.surface_hi,
        );
    }
    ty.text(c, Style::bold(s(48) as f32, t.rust), "<", s(64), s(178));
    let title_style = Style::bold(s(54) as f32, t.fg);
    ty.text(c, title_style, title, s(150), s(178));
    if let Some(status) = status {
        let style = Style::regular(s(26) as f32, t.fg_dim);
        let max = c.width() - s(150) - ty.width(title_style, title) - s(100);
        let text = ellipsize(ty, style, status, max);
        let w = ty.width(style, &text);
        ty.text(c, style, &text, c.width() - s(48) - w, s(178));
    }
    c.fill_rect(
        Rect {
            x: s(48),
            y: s(230),
            w: c.width() - s(96),
            h: hairline(b),
        },
        t.rust_dim,
    );
}

/// Section label: uppercase, letter-spaced feel through bold small type.
pub fn section_label(c: &mut Canvas, ty: &Type, text: &str, x: i32, baseline: i32) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    ty.text(
        c,
        Style::bold(scale(26, b) as f32, t.fg_dim),
        &text.to_uppercase(),
        x,
        baseline,
    );
}

/// Shortens `text` with `…` until it fits `max_width`.
pub fn ellipsize(ty: &Type, style: Style, text: &str, max_width: i32) -> String {
    if ty.width(style, text) <= max_width {
        return text.to_owned();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "\u{2026}";
        if ty.width(style, &candidate) <= max_width {
            return candidate;
        }
    }
    String::new()
}

/// Word-wraps `text` at `max_width`, returning the lines.
pub fn wrap(ty: &Type, style: Style, text: &str, max_width: i32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let candidate = if line.is_empty() {
            word.to_owned()
        } else {
            format!("{line} {word}")
        };
        if !line.is_empty() && ty.width(style, &candidate) > max_width {
            lines.push(std::mem::take(&mut line));
            line = word.to_owned();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// Draws wrapped `fg_dim` body text; returns the baseline after the last line.
pub fn wrap_text(
    c: &mut Canvas,
    ty: &Type,
    text: &str,
    x: i32,
    y: i32,
    max_width: i32,
    px: i32,
) -> i32 {
    let style = Style::regular(px as f32, theme::get().fg_dim);
    let mut baseline = y;
    for line in wrap(ty, style, text, max_width) {
        ty.text(c, style, &line, x, baseline);
        baseline += px * 3 / 2;
    }
    baseline
}

/// Tone of a chip or notice: which palette entry carries its state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Slate chip, `fg` text: an inactive or informational state.
    Neutral,
    /// The rust accent: the active or selected state of a region.
    Accent,
    /// Steel: network and secondary emphasis.
    Steel,
    Ok,
    Warn,
    Err,
}

impl Tone {
    pub fn color(self) -> u32 {
        let t = theme::get();
        match self {
            Tone::Neutral => t.fg_dim,
            Tone::Accent => t.rust,
            Tone::Steel => t.steel,
            Tone::Ok => t.ok,
            Tone::Warn => t.warn,
            Tone::Err => t.err,
        }
    }
}

/// A small state chip: 6 px radius, hairline border in the tone color,
/// tone-colored text. Pressed raises the fill.
pub fn chip(c: &mut Canvas, ty: &Type, r: Rect, label: &str, tone: Tone, pressed: bool) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    c.fill_rounded(r, s(6), if pressed { t.surface_hi } else { t.surface });
    let outline = if tone == Tone::Neutral {
        t.border
    } else {
        tone.color()
    };
    c.stroke_rounded(r, s(6), hairline(b), outline);
    let color = if tone == Tone::Neutral {
        t.fg
    } else {
        tone.color()
    };
    let mut style = Style::bold((r.h as f32 * 0.42).max(12.0), color);
    let text = ellipsize(ty, style, label, r.w - s(24));
    if ty.width(style, &text) > r.w - s(16) {
        style.px = (style.px * 0.8).max(10.0);
    }
    let w = ty.width(style, &text);
    ty.text(
        c,
        style,
        &text,
        r.x + (r.w - w) / 2,
        r.y + r.h / 2 + (style.px * 0.36) as i32,
    );
}

/// Button kinds: primary carries the region's rust accent as an outline
/// and rust label; danger uses `err`; secondary is a slate card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Danger,
}

/// A large touch button. Disabled buttons keep their shape with `fg_dim`
/// text and no accent so the reason line beside them carries the state.
pub fn button(
    c: &mut Canvas,
    ty: &Type,
    r: Rect,
    label: &str,
    kind: ButtonKind,
    pressed: bool,
    enabled: bool,
) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    c.fill_rounded(r, s(10), if pressed { t.surface_hi } else { t.surface });
    let (thickness, outline, color) = match (kind, enabled, pressed) {
        (_, false, _) => (hairline(b), t.border, t.fg_dim),
        (ButtonKind::Primary, true, true) => (s(3), t.rust_hi, t.rust_hi),
        (ButtonKind::Primary, true, false) => (s(3), t.rust, t.rust),
        (ButtonKind::Danger, true, _) => (s(3), t.err, t.err),
        (ButtonKind::Secondary, true, true) => (s(3), t.rust, t.fg),
        (ButtonKind::Secondary, true, false) => (hairline(b), t.border, t.fg),
    };
    c.stroke_rounded(r, s(10), thickness, outline);
    let style = Style::bold((r.h as f32 * 0.36).max(14.0), color);
    let text = ellipsize(ty, style, label, r.w - s(32));
    let w = ty.width(style, &text);
    ty.text(
        c,
        style,
        &text,
        r.x + (r.w - w) / 2,
        r.y + r.h / 2 + (style.px * 0.36) as i32,
    );
}

/// Row rectangles for a vertical list starting at `top`: full width minus
/// the 48 px margin, `row_h` design px each with a 12 px gap.
pub fn list_rects(size: (u32, u32), top: i32, count: usize, row_h: i32) -> Vec<Rect> {
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    let margin = s(48);
    let gap = s(12);
    let h = s(row_h);
    (0..count)
        .map(|i| Rect {
            x: margin,
            y: top + i as i32 * (h + gap),
            w: size.0 as i32 - 2 * margin,
            h,
        })
        .collect()
}

/// A list row: title in `fg`, subtitle in `fg_dim`, optional trailing chip
/// text in the given tone. Pressed raises the fill and outlines in rust.
pub fn list_row(
    c: &mut Canvas,
    ty: &Type,
    r: Rect,
    title: &str,
    subtitle: &str,
    trailing: Option<(&str, Tone)>,
    pressed: bool,
) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    c.fill_rounded(r, s(10), if pressed { t.surface_hi } else { t.surface });
    c.stroke_rounded(
        r,
        s(10),
        if pressed { s(3) } else { hairline(b) },
        if pressed { t.rust } else { t.border },
    );
    let mut trailing_w = 0;
    if let Some((label, tone)) = trailing {
        let style = Style::bold(s(24) as f32, tone.color());
        let w = ty.width(style, label) + s(28);
        let chip_r = Rect {
            x: r.x + r.w - s(24) - w,
            y: r.y + (r.h - s(48)) / 2,
            w,
            h: s(48),
        };
        chip(c, ty, chip_r, label, tone, false);
        trailing_w = w + s(36);
    }
    let title_style = Style::bold(s(34) as f32, t.fg);
    let max = r.w - s(64) - trailing_w;
    let has_sub = !subtitle.is_empty();
    let title_base = if has_sub {
        r.y + r.h / 2 - s(6)
    } else {
        r.y + r.h / 2 + s(12)
    };
    ty.text(
        c,
        title_style,
        &ellipsize(ty, title_style, title, max),
        r.x + s(32),
        title_base,
    );
    if has_sub {
        let sub_style = Style::regular(s(26) as f32, t.fg_dim);
        ty.text(
            c,
            sub_style,
            &ellipsize(ty, sub_style, subtitle, max),
            r.x + s(32),
            r.y + r.h / 2 + s(38),
        );
    }
}

/// A single-line text field. Focused fields take the rust outline and a
/// block cursor; an empty field shows `placeholder` in `fg_dim`.
pub fn text_field(
    c: &mut Canvas,
    ty: &Type,
    r: Rect,
    text: &str,
    placeholder: &str,
    focused: bool,
) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    c.fill_rounded(r, s(8), t.bg_dark);
    c.stroke_rounded(
        r,
        s(8),
        if focused { s(3) } else { hairline(b) },
        if focused { t.rust } else { t.border },
    );
    let px = (r.h as f32 * 0.42).max(14.0);
    let baseline = r.y + r.h / 2 + (px * 0.36) as i32;
    let max = r.w - s(64);
    if text.is_empty() {
        let style = Style::regular(px, t.fg_dim);
        ty.text(
            c,
            style,
            &ellipsize(ty, style, placeholder, max),
            r.x + s(28),
            baseline,
        );
        if focused {
            c.fill_rect(
                Rect {
                    x: r.x + s(28),
                    y: baseline - px as i32,
                    w: s(4),
                    h: (px * 1.2) as i32,
                },
                t.rust,
            );
        }
        return;
    }
    let style = Style::regular(px, t.fg);
    // Keep the tail visible while typing.
    let mut shown: String = text.to_owned();
    while ty.width(style, &shown) > max && shown.chars().count() > 1 {
        shown.remove(0);
    }
    ty.text(c, style, &shown, r.x + s(28), baseline);
    if focused {
        let x = r.x + s(28) + ty.width(style, &shown) + s(4);
        c.fill_rect(
            Rect {
                x,
                y: baseline - px as i32,
                w: s(4),
                h: (px * 1.2) as i32,
            },
            t.rust,
        );
    }
}

/// A one-line notice with a colored rule at the left: service status,
/// typed errors, reconnection notes. Never decorative.
pub fn notice(c: &mut Canvas, ty: &Type, r: Rect, text: &str, tone: Tone) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    c.fill_rounded(r, s(10), t.surface);
    c.stroke_rounded(r, s(10), hairline(b), t.border);
    c.fill_rounded(
        Rect {
            x: r.x + s(18),
            y: r.y + s(20),
            w: s(8),
            h: r.h - s(40),
        },
        s(2),
        tone.color(),
    );
    let mut style = Style::regular(s(28) as f32, t.fg);
    let max = r.w - s(72);
    while ty.width(style, text) > max && style.px > 12.0 {
        style.px -= 2.0;
    }
    ty.text(
        c,
        style,
        text,
        r.x + s(48),
        r.y + r.h / 2 + (style.px * 0.36) as i32,
    );
}

/// Standard telephone keypad order.
pub const KEYPAD: [char; 12] = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '*', '0', '#'];

/// Keypad geometry: three columns by four rows from `top`, keys `key` design
/// px tall with a 16 px gap, centered horizontally.
pub fn keypad_rects(size: (u32, u32), top: i32, key_h: i32) -> Vec<(Rect, char)> {
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    let gap = s(16);
    let key_w = (size.0 as i32 - 2 * s(96) - 2 * gap) / 3;
    let h = s(key_h);
    KEYPAD
        .iter()
        .enumerate()
        .map(|(i, ch)| {
            let col = (i % 3) as i32;
            let row = (i / 3) as i32;
            (
                Rect {
                    x: s(96) + col * (key_w + gap),
                    y: top + row * (h + gap),
                    w: key_w,
                    h,
                },
                *ch,
            )
        })
        .collect()
}

/// Draws the keypad; the pressed key takes the raised fill and rust outline.
pub fn keypad(c: &mut Canvas, ty: &Type, keys: &[(Rect, char)], pressed: Option<char>) {
    let t = theme::get();
    let b = basis((c.width() as u32, c.height() as u32));
    let s = |v: i32| scale(v, b);
    for (r, ch) in keys {
        let is_pressed = pressed == Some(*ch);
        c.fill_rounded(*r, s(10), if is_pressed { t.surface_hi } else { t.surface });
        c.stroke_rounded(
            *r,
            s(10),
            if is_pressed { s(3) } else { hairline(b) },
            if is_pressed { t.rust } else { t.border },
        );
        let style = Style::regular((r.h as f32 * 0.5).max(16.0), t.fg);
        let label = ch.to_string();
        let w = ty.width(style, &label);
        ty.text(
            c,
            style,
            &label,
            r.x + (r.w - w) / 2,
            r.y + r.h / 2 + (style.px * 0.36) as i32,
        );
    }
}

/// The shared touch keyboard tray (`osk` geometry) in the system look:
/// `bg_dark` well, character keys on `surface`, function keys on `bg`,
/// sticky modifiers in rust.
pub fn keyboard(c: &mut Canvas, ty: &Type, state: &OskState, pressed: Option<Key>) {
    let t = theme::get();
    let size = (c.width() as u32, c.height() as u32);
    let b = basis(size);
    let s = |v: i32| scale(v, b);
    let hair = hairline(b);
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
    for (r, key) in osk::keys(size, state) {
        let is_pressed = pressed == Some(key);
        let active = (key == Key::Shift && state.shift) || (key == Key::Ctrl && state.ctrl);
        let function = osk::is_function(key);
        let fill = if is_pressed {
            t.surface_hi
        } else if active {
            t.rust
        } else if function {
            t.bg
        } else {
            t.surface
        };
        c.fill_rounded(r, s(6), fill);
        c.stroke_rounded(r, s(6), hair, if is_pressed { t.rust } else { t.border });
        let label = osk::label(key, state);
        if label.is_empty() {
            continue;
        }
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
        ty.text(
            c,
            style,
            &label,
            r.x + (r.w - lw) / 2,
            r.y + r.h / 2 + (px * 0.36) as i32,
        );
    }
}

/// Outcome of feeding one keyboard key into a text buffer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Unchanged,
    Changed,
    /// Enter was pressed.
    Submit,
    /// Escape was pressed.
    Dismiss,
}

/// Applies a touch-keyboard key to `text`: characters insert (honouring the
/// sticky shift), backspace deletes, enter submits, modifiers and layers
/// update `osk` only. Terminal-only keys are ignored.
pub fn edit_text(text: &mut String, key: Key, osk: &mut OskState) -> Edit {
    match key {
        Key::Shift | Key::Ctrl | Key::Layer => {
            osk.press(key, false);
            Edit::Unchanged
        }
        Key::Backspace => {
            osk.ctrl = false;
            if text.pop().is_some() {
                Edit::Changed
            } else {
                Edit::Unchanged
            }
        }
        Key::Enter => {
            osk.ctrl = false;
            Edit::Submit
        }
        Key::Esc => Edit::Dismiss,
        Key::Space => {
            osk.ctrl = false;
            text.push(' ');
            Edit::Changed
        }
        Key::Char(ch) => {
            let upper = osk.shift && !osk.symbols;
            osk.ctrl = false;
            if !osk.symbols {
                osk.shift = false;
            }
            text.push(if upper { ch.to_ascii_uppercase() } else { ch });
            Edit::Changed
        }
        _ => Edit::Unchanged,
    }
}

/// `name · state` copy rule helper.
pub fn state_line(name: &str, state: &str) -> String {
    format!("{name} \u{b7} {state}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1080, 2340);

    #[test]
    fn geometry_is_shared_and_inside_the_panel() {
        let keys = keypad_rects(SIZE, 900, 150);
        assert_eq!(keys.len(), 12);
        assert_eq!(keys[0].1, '1');
        assert_eq!(keys[10].1, '0');
        for (r, _) in &keys {
            assert!(r.x >= 0 && r.x + r.w <= 1080 && r.y >= 900);
        }
        assert_eq!(keys[1].0.y, keys[0].0.y);
        assert_eq!(keys[3].0.x, keys[0].0.x);
        let rows = list_rects(SIZE, 300, 3, 140);
        assert_eq!(rows[1].y, 300 + 140 + 12);
        assert_eq!(rows[0].w, 1080 - 96);
        assert!(contains(back_rect(SIZE), (80.0, 160.0)));
        assert!(!contains(back_rect(SIZE), (600.0, 160.0)));
        assert_eq!(header_bottom(1080), 252);
        assert_eq!(scale(72, 830), 55);
        assert_eq!(hairline(540), 1);
    }

    #[test]
    fn widgets_render_their_state_colors() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let mut c = Canvas::new(1080, 2340, t.bg);
        clock_strip(&mut c, &ty, "09:41", Some("telephony \u{b7} mock"));
        assert_eq!(c.get(540, 10), t.bg_dark);
        assert_eq!(c.get(540, clock_strip_height(1080)), t.rust_dim);
        page_header(&mut c, &ty, "Phone", Some("status"), false);
        assert_eq!(c.get(540, 230), t.rust_dim);
        let r = Rect {
            x: 100,
            y: 400,
            w: 300,
            h: 60,
        };
        chip(&mut c, &ty, r, "connected", Tone::Ok, false);
        assert_eq!(c.get(250, 400), t.ok, "chip outline carries the tone");
        chip(
            &mut c,
            &ty,
            Rect { y: 500, ..r },
            "idle",
            Tone::Neutral,
            false,
        );
        assert_eq!(c.get(250, 500), t.border);
        let b = Rect {
            x: 100,
            y: 600,
            w: 400,
            h: 120,
        };
        button(&mut c, &ty, b, "Call", ButtonKind::Primary, false, true);
        assert_eq!(c.get(300, 600), t.rust);
        button(
            &mut c,
            &ty,
            Rect { y: 800, ..b },
            "Call",
            ButtonKind::Primary,
            false,
            false,
        );
        assert_eq!(c.get(300, 800), t.border, "disabled loses the accent");
        button(
            &mut c,
            &ty,
            Rect { y: 1000, ..b },
            "Delete",
            ButtonKind::Danger,
            false,
            true,
        );
        assert_eq!(c.get(300, 1000), t.err);
        let f = Rect {
            x: 100,
            y: 1200,
            w: 800,
            h: 100,
        };
        text_field(&mut c, &ty, f, "", "Search", true);
        assert_eq!(c.get(500, 1200), t.rust);
        assert_eq!(c.get(500, 1250), t.bg_dark);
        text_field(&mut c, &ty, Rect { y: 1400, ..f }, "abc", "Search", false);
        assert_eq!(c.get(500, 1400), t.border);
        list_row(
            &mut c,
            &ty,
            Rect {
                x: 48,
                y: 1600,
                w: 984,
                h: 140,
            },
            "Test Contact 01",
            "mobile",
            Some(("read", Tone::Steel)),
            true,
        );
        assert_eq!(c.get(540, 1600), t.rust, "pressed row outline");
        notice(
            &mut c,
            &ty,
            Rect {
                x: 48,
                y: 1800,
                w: 984,
                h: 100,
            },
            "telephony \u{b7} unavailable",
            Tone::Warn,
        );
        assert_eq!(c.get(48 + 22, 1850), t.warn);
        let keys = keypad_rects(SIZE, 1950, 100);
        keypad(&mut c, &ty, &keys, Some('5'));
        let (five, _) = keys[4];
        assert_eq!(c.get(five.x + five.w / 2, five.y), t.rust);
        let (one, _) = keys[0];
        assert_eq!(c.get(one.x + one.w / 2, one.y + 6), t.surface);
        gesture_pill(&mut c, false);
        assert_eq!(c.get(540, 2340 - 54), t.fg_dim);
    }

    #[test]
    fn keyboard_tray_and_text_editing() {
        let t = theme::get();
        let ty = Type::load().unwrap();
        let mut c = Canvas::new(1080, 2340, t.bg);
        let mut osk = OskState::default();
        keyboard(&mut c, &ty, &osk, None);
        let keys = osk::keys(SIZE, &osk);
        let (q, _) = keys.iter().find(|(_, k)| *k == Key::Char('q')).unwrap();
        assert_eq!(c.get(q.x + 6, q.y + 6), t.surface);
        let (enter, _) = keys.iter().find(|(_, k)| *k == Key::Enter).unwrap();
        assert_eq!(c.get(enter.x + 6, enter.y + 6), t.bg);

        let mut text = String::new();
        assert_eq!(edit_text(&mut text, Key::Shift, &mut osk), Edit::Unchanged);
        assert_eq!(
            edit_text(&mut text, Key::Char('t'), &mut osk),
            Edit::Changed
        );
        assert_eq!(
            edit_text(&mut text, Key::Char('e'), &mut osk),
            Edit::Changed
        );
        assert_eq!(edit_text(&mut text, Key::Space, &mut osk), Edit::Changed);
        assert_eq!(
            edit_text(&mut text, Key::Backspace, &mut osk),
            Edit::Changed
        );
        assert_eq!(text, "Te");
        assert_eq!(edit_text(&mut text, Key::Enter, &mut osk), Edit::Submit);
        assert_eq!(edit_text(&mut text, Key::Esc, &mut osk), Edit::Dismiss);
        assert_eq!(edit_text(&mut text, Key::Up, &mut osk), Edit::Unchanged);
        text.clear();
        assert_eq!(
            edit_text(&mut text, Key::Backspace, &mut osk),
            Edit::Unchanged
        );
        assert_eq!(
            state_line("telephony", "reconnected"),
            "telephony \u{b7} reconnected"
        );
        let style = Style::regular(30.0, t.fg);
        assert_eq!(ellipsize(&ty, style, "short", 1000), "short");
        let cut = ellipsize(&ty, style, "a very long line that does not fit", 200);
        assert!(cut.ends_with('\u{2026}') && cut.chars().count() < 20);
        assert!(wrap(&ty, style, "one two three four five six", 200).len() > 1);
    }
}
