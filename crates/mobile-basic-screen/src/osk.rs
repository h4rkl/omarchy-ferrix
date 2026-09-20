//! Touch-first on-screen keyboard for the Terminal app.
//!
//! Geometry is pure and shared by rendering and hit testing, so the
//! interaction tests exercise the same rectangles the framebuffer shows.
//! Layers follow the standard phone convention (letters, `?123` symbols, and a
//! shifted symbol row) from the GrapheneOS fallback reference; the accessory
//! row adds the keys a shell needs (esc, tab, ctrl, backtick, backslash, pipe,
//! arrows). Shift and ctrl are sticky for one key, as on a phone keyboard.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::draw::Rect;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Shift,
    Ctrl,
    /// Toggle between the letter and symbol layers.
    Layer,
    Backspace,
    Enter,
    Space,
    Esc,
    Tab,
    Up,
    Down,
    Left,
    Right,
    // Hardware-keyboard only; not on the touch layout.
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OskState {
    pub shift: bool,
    pub ctrl: bool,
    pub symbols: bool,
}

impl OskState {
    /// Applies a key press: modifier and layer keys update the sticky state and
    /// return nothing; every other key returns the bytes to write to the PTY.
    pub fn press(&mut self, key: Key, app_cursor: bool) -> Option<Vec<u8>> {
        match key {
            Key::Shift => {
                self.shift = !self.shift;
                None
            }
            Key::Ctrl => {
                self.ctrl = !self.ctrl;
                None
            }
            Key::Layer => {
                self.symbols = !self.symbols;
                self.shift = false;
                None
            }
            _ => {
                let upper = self.shift && !self.symbols;
                let bytes = bytes(key, upper, self.ctrl, app_cursor);
                self.ctrl = false;
                if !self.symbols {
                    self.shift = false;
                }
                bytes
            }
        }
    }
}

/// Byte sequence for a key, xterm style. `upper` applies to letters, `ctrl`
/// produces C0 controls, and arrows honor DECCKM application cursor mode.
pub fn bytes(key: Key, upper: bool, ctrl: bool, app_cursor: bool) -> Option<Vec<u8>> {
    let arrow = |code: u8| {
        Some(if app_cursor {
            vec![0x1b, b'O', code]
        } else {
            vec![0x1b, b'[', code]
        })
    };
    match key {
        Key::Char(c) => {
            let c = if upper { c.to_ascii_uppercase() } else { c };
            if ctrl && let Some(control) = control_byte(c) {
                return Some(vec![control]);
            }
            let mut buf = [0u8; 4];
            Some(c.encode_utf8(&mut buf).as_bytes().to_vec())
        }
        Key::Space => Some(vec![if ctrl { 0x00 } else { b' ' }]),
        Key::Enter => Some(vec![b'\r']),
        Key::Backspace => Some(vec![0x7f]),
        Key::Tab => Some(vec![b'\t']),
        Key::Esc => Some(vec![0x1b]),
        Key::Up => arrow(b'A'),
        Key::Down => arrow(b'B'),
        Key::Right => arrow(b'C'),
        Key::Left => arrow(b'D'),
        Key::Home => Some(b"\x1b[H".to_vec()),
        Key::End => Some(b"\x1b[F".to_vec()),
        Key::Delete => Some(b"\x1b[3~".to_vec()),
        Key::PageUp => Some(b"\x1b[5~".to_vec()),
        Key::PageDown => Some(b"\x1b[6~".to_vec()),
        Key::Shift | Key::Ctrl | Key::Layer => None,
    }
}

fn control_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' => Some(c as u8 - b'a' + 1),
        'A'..='Z' => Some(c as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

const LETTER_ROWS: [&str; 3] = ["qwertyuiop", "asdfghjkl", "zxcvbnm"];
const SYMBOL_ROWS: [&str; 3] = ["1234567890", "-/:;()$&@\"", "_=+*'!?"];
/// Shift in the symbol layer swaps the digit row for the remaining ASCII
/// punctuation, so every printable ASCII character is reachable by touch.
const SHIFTED_SYMBOL_ROW: &str = "[]{}#%^~<>";
const ACCESSORY_ROW: [Key; 10] = [
    Key::Esc,
    Key::Tab,
    Key::Ctrl,
    Key::Char('`'),
    Key::Char('\\'),
    Key::Char('|'),
    Key::Left,
    Key::Down,
    Key::Up,
    Key::Right,
];

/// Rows are authored in units of one tenth of the panel width.
const ROW_UNITS: f64 = 10.0;
/// Row height as a fraction of panel width (108px on the 1080 panel).
const ROW_HEIGHT: f64 = 0.1;
/// Keyboard bottom edge as a fraction of panel height, just above the
/// gesture-pill zone that begins at 92%.
const BOTTOM: f64 = 0.915;
/// Spacing between keys as a fraction of panel width.
const GAP: f64 = 0.007;
const ROWS: usize = 5;

/// Vertical extent `(top, bottom)` of the keyboard in pixels.
pub fn extent(size: (u32, u32)) -> (i32, i32) {
    let (w, h) = (f64::from(size.0), f64::from(size.1));
    let bottom = h * BOTTOM;
    let top = bottom - ROWS as f64 * w * ROW_HEIGHT;
    (top.round() as i32, bottom.round() as i32)
}

/// Terminal grid panel: below the page header rule (230 design px) and above
/// the keyboard, with the launcher's horizontal margin.
pub fn terminal_area(size: (u32, u32)) -> Rect {
    let w = size.0 as i32;
    let design = |v: i32| v * w / 1080;
    let (kb_top, _) = extent(size);
    let top = design(252);
    Rect {
        x: design(24),
        y: top,
        w: w - design(48),
        h: (kb_top - design(16) - top).max(1),
    }
}

/// Key label in the current layer. Letters follow the shift state.
pub fn label(key: Key, state: &OskState) -> String {
    match key {
        Key::Char(c) if state.shift && !state.symbols => c.to_ascii_uppercase().to_string(),
        Key::Char(c) => c.to_string(),
        Key::Shift => "\u{21e7}".to_owned(),
        Key::Ctrl => "ctrl".to_owned(),
        Key::Layer if state.symbols => "abc".to_owned(),
        Key::Layer => "?123".to_owned(),
        Key::Backspace => "\u{232b}".to_owned(),
        Key::Enter => "\u{23ce}".to_owned(),
        Key::Space => String::new(),
        Key::Esc => "esc".to_owned(),
        Key::Tab => "tab".to_owned(),
        Key::Up => "\u{2191}".to_owned(),
        Key::Down => "\u{2193}".to_owned(),
        Key::Left => "\u{2190}".to_owned(),
        Key::Right => "\u{2192}".to_owned(),
        Key::Delete | Key::Home | Key::End | Key::PageUp | Key::PageDown => String::new(),
    }
}

/// Function keys get the darker key face; characters get the standard surface.
pub fn is_function(key: Key) -> bool {
    !matches!(key, Key::Char(_) | Key::Space)
}

fn rows(state: &OskState) -> Vec<Vec<(f64, Option<Key>)>> {
    let chars = |row: &str| -> Vec<(f64, Option<Key>)> {
        row.chars().map(|c| (1.0, Some(Key::Char(c)))).collect()
    };
    let mut row1;
    let row2;
    let mut row3;
    if state.symbols {
        row1 = chars(if state.shift {
            SHIFTED_SYMBOL_ROW
        } else {
            SYMBOL_ROWS[0]
        });
        row2 = chars(SYMBOL_ROWS[1]);
        row3 = chars(SYMBOL_ROWS[2]);
    } else {
        row1 = chars(LETTER_ROWS[0]);
        let mut middle = vec![(0.5, None)];
        middle.extend(chars(LETTER_ROWS[1]));
        middle.push((0.5, None));
        row2 = middle;
        row3 = chars(LETTER_ROWS[2]);
    }
    row1.truncate(10);
    row3.insert(0, (1.5, Some(Key::Shift)));
    row3.push((1.5, Some(Key::Backspace)));
    let row4 = vec![
        (1.5, Some(Key::Layer)),
        (1.0, Some(Key::Char(','))),
        (5.0, Some(Key::Space)),
        (1.0, Some(Key::Char('.'))),
        (1.5, Some(Key::Enter)),
    ];
    let accessory = ACCESSORY_ROW.iter().map(|k| (1.0, Some(*k))).collect();
    vec![accessory, row1, row2, row3, row4]
}

/// Key rectangles for the current layer, in panel pixels.
pub fn keys(size: (u32, u32), state: &OskState) -> Vec<(Rect, Key)> {
    let w = f64::from(size.0);
    let unit = w / ROW_UNITS;
    let row_h = w * ROW_HEIGHT;
    let gap = w * GAP;
    let (top, _) = extent(size);
    let mut out = Vec::new();
    for (row_index, row) in rows(state).into_iter().enumerate() {
        let y = f64::from(top) + row_index as f64 * row_h;
        let mut x = 0.0;
        for (units, key) in row {
            let kw = units * unit;
            if let Some(key) = key {
                out.push((
                    Rect {
                        x: (x + gap / 2.0).round() as i32,
                        y: (y + gap / 2.0).round() as i32,
                        w: (kw - gap).round() as i32,
                        h: (row_h - gap).round() as i32,
                    },
                    key,
                ));
            }
            x += kw;
        }
    }
    out
}

pub fn contains(r: Rect, position: (f64, f64)) -> bool {
    let (x, y) = position;
    x >= f64::from(r.x)
        && x < f64::from(r.x + r.w)
        && y >= f64::from(r.y)
        && y < f64::from(r.y + r.h)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SIZE: (u32, u32) = (1080, 2340);

    #[test]
    fn layout_stays_inside_the_panel_and_keys_do_not_overlap() {
        for state in [
            OskState::default(),
            OskState {
                symbols: true,
                ..OskState::default()
            },
            OskState {
                symbols: true,
                shift: true,
                ..OskState::default()
            },
        ] {
            let keys = keys(SIZE, &state);
            // Accessory, top row, middle row (9 letters or 10 symbols),
            // shift/backspace row, and the space row.
            let middle = if state.symbols { 10 } else { 9 };
            assert_eq!(keys.len(), 10 + 10 + middle + 9 + 5, "{state:?}");
            let (top, bottom) = extent(SIZE);
            for (i, (r, _)) in keys.iter().enumerate() {
                assert!(r.x >= 0 && r.x + r.w <= 1080 && r.y >= top && r.y + r.h <= bottom);
                assert!(
                    r.y + r.h < (2340.0 * 0.92) as i32,
                    "keys stay above Home zone"
                );
                for (r2, _) in &keys[i + 1..] {
                    let overlap = r.x < r2.x + r2.w
                        && r2.x < r.x + r.w
                        && r.y < r2.y + r2.h
                        && r2.y < r.y + r.h;
                    assert!(!overlap, "{r:?} overlaps {r2:?}");
                }
            }
            let area = terminal_area(SIZE);
            assert!(area.y + area.h <= top);
            assert!(area.h > 600, "terminal keeps most of the free height");
        }
    }

    #[test]
    fn every_printable_ascii_character_is_reachable() {
        let mut seen = std::collections::HashSet::new();
        for state in [
            OskState::default(),
            OskState {
                shift: true,
                ..OskState::default()
            },
            OskState {
                symbols: true,
                ..OskState::default()
            },
            OskState {
                symbols: true,
                shift: true,
                ..OskState::default()
            },
        ] {
            for (_, key) in keys(SIZE, &state) {
                if let Key::Char(c) = key {
                    let c = if state.shift && !state.symbols {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    };
                    seen.insert(c);
                }
                if key == Key::Space {
                    seen.insert(' ');
                }
            }
        }
        for c in 0x20u8..0x7f {
            assert!(seen.contains(&(c as char)), "missing {:?}", c as char);
        }
    }

    #[test]
    fn sticky_modifiers_and_control_bytes() {
        let mut state = OskState::default();
        assert_eq!(state.press(Key::Shift, false), None);
        assert_eq!(state.press(Key::Char('a'), false), Some(b"A".to_vec()));
        assert!(!state.shift, "shift releases after one letter");
        assert_eq!(state.press(Key::Ctrl, false), None);
        assert_eq!(state.press(Key::Char('c'), false), Some(vec![0x03]));
        assert!(!state.ctrl);
        assert_eq!(state.press(Key::Enter, false), Some(b"\r".to_vec()));
        assert_eq!(state.press(Key::Backspace, false), Some(vec![0x7f]));
        assert_eq!(state.press(Key::Up, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(state.press(Key::Up, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(state.press(Key::Layer, false), None);
        assert!(state.symbols);
        assert_eq!(state.press(Key::Shift, false), None);
        assert_eq!(state.press(Key::Char('#'), false), Some(b"#".to_vec()));
        assert!(state.shift, "shift is a layer toggle for symbols");
    }
}
