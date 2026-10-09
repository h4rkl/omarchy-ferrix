//! Touch-first on-screen keyboard shared by every app.
//!
//! Geometry is pure and shared by rendering and hit testing, so the
//! interaction tests exercise the same rectangles the framebuffer shows.
//! Each input field declares a [`Layout`]:
//!
//! - [`Layout::Text`]: letters, shift, a `?123` symbols page, space,
//!   backspace, and enter, for names, message bodies, and passphrases.
//! - [`Layout::Phone`]: a 12-key dial pad plus `+`, backspace, and enter, for
//!   phone-number fields.
//! - [`Layout::Terminal`]: the text layers plus an accessory row with the keys
//!   a shell needs (esc, tab, ctrl, backtick, backslash, pipe, arrows).
//!
//! Layers follow the standard phone convention (letters, `?123` symbols, and a
//! shifted symbol page) from the GrapheneOS fallback reference. Shift and ctrl
//! are sticky for one key, as on a phone keyboard; tapping shift again while
//! it is armed locks capitals until the next shift tap. The sequence (not a
//! tap timer) keeps the state machine deterministic and testable. Every
//! layout fills the same tray height, so focusing a different field never
//! moves the content above the keyboard.
use crate::draw::Rect;

/// Which key set a focused input field asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Layout {
    /// General text: letters, symbols page, space, backspace, enter.
    #[default]
    Text,
    /// Dial pad: `0`-`9`, `*`, `#`, `+`, backspace, enter.
    Phone,
    /// Text layers plus the shell accessory row.
    Terminal,
}

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
    pub layout: Layout,
    /// One-shot shift for letters, or the shifted page in the symbol layer.
    pub shift: bool,
    /// Capitals stay on until shift is tapped again (letters layer only).
    pub caps_lock: bool,
    pub ctrl: bool,
    pub symbols: bool,
}

impl OskState {
    pub fn new(layout: Layout) -> Self {
        Self {
            layout,
            ..Self::default()
        }
    }

    /// The same modifier state shown with another layout. Leaving the letter
    /// layers drops the sticky modifiers so they cannot leak into a dial pad.
    pub fn with_layout(self, layout: Layout) -> Self {
        if layout == self.layout {
            self
        } else {
            Self::new(layout)
        }
    }

    /// Whether letter keys currently produce (and show) capitals.
    pub fn upper(&self) -> bool {
        (self.shift || self.caps_lock) && !self.symbols && self.layout != Layout::Phone
    }

    /// Whether the shift key is drawn active.
    pub fn shift_active(&self) -> bool {
        self.shift || (self.caps_lock && !self.symbols)
    }

    /// Updates sticky state for a shift tap: off, then one-shot shift, then
    /// caps lock, then off again. The symbol layer toggles its shifted page.
    fn tap_shift(&mut self) {
        if self.symbols {
            self.shift = !self.shift;
        } else if self.caps_lock {
            self.caps_lock = false;
            self.shift = false;
        } else if self.shift {
            self.shift = false;
            self.caps_lock = true;
        } else {
            self.shift = true;
        }
    }

    /// Case for the character key being typed now; consumes a one-shot shift
    /// (and ctrl) in the letters layer.
    pub fn take_char_case(&mut self) -> bool {
        let upper = self.upper();
        self.ctrl = false;
        if !self.symbols {
            self.shift = false;
        }
        upper
    }

    /// Applies a key press: modifier and layer keys update the sticky state and
    /// return nothing; every other key returns the bytes to write to the PTY.
    pub fn press(&mut self, key: Key, app_cursor: bool) -> Option<Vec<u8>> {
        match key {
            Key::Shift => {
                self.tap_shift();
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
                let ctrl = self.ctrl;
                let upper = self.take_char_case();
                bytes(key, upper, ctrl, app_cursor)
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
/// Without the terminal accessory row, the shifted symbol page also carries
/// backtick, backslash, and pipe so passphrases can use any printable ASCII.
const TEXT_SHIFTED_SYMBOL_ROW3: &str = "`\\|_=+*";
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
/// Dial pad rows, ITU E.161 order, with the international prefix and the
/// edit keys on the last row.
const PHONE_ROWS: [[Key; 3]; 5] = [
    [Key::Char('1'), Key::Char('2'), Key::Char('3')],
    [Key::Char('4'), Key::Char('5'), Key::Char('6')],
    [Key::Char('7'), Key::Char('8'), Key::Char('9')],
    [Key::Char('*'), Key::Char('0'), Key::Char('#')],
    [Key::Char('+'), Key::Backspace, Key::Enter],
];

/// Rows are authored in units of one tenth of the panel width.
const ROW_UNITS: f64 = 10.0;
/// Terminal row height as a fraction of panel width (108px on the 1080
/// panel); it sets the tray height every layout shares.
const ROW_HEIGHT: f64 = 0.1;
/// Keyboard bottom edge as a fraction of panel height, just above the
/// gesture-pill zone that begins at 92%.
const BOTTOM: f64 = 0.915;
/// Spacing between keys as a fraction of panel width.
const GAP: f64 = 0.007;
/// Rows in the tallest (terminal) layout; the tray is sized for it.
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
        Key::Char(c) if state.upper() => c.to_ascii_uppercase().to_string(),
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

type Row = Vec<(f64, Option<Key>)>;

fn chars(row: &str) -> Row {
    row.chars().map(|c| (1.0, Some(Key::Char(c)))).collect()
}

/// Letter or symbol layers shared by the text and terminal layouts.
fn text_rows(state: &OskState) -> Vec<Row> {
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
        row3 = chars(if state.shift && state.layout == Layout::Text {
            TEXT_SHIFTED_SYMBOL_ROW3
        } else {
            SYMBOL_ROWS[2]
        });
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
    vec![row1, row2, row3, row4]
}

fn rows(state: &OskState) -> Vec<Row> {
    match state.layout {
        Layout::Text => text_rows(state),
        Layout::Terminal => {
            let accessory = ACCESSORY_ROW.iter().map(|k| (1.0, Some(*k))).collect();
            let mut rows = vec![accessory];
            rows.extend(text_rows(state));
            rows
        }
        Layout::Phone => PHONE_ROWS
            .iter()
            .map(|row| row.iter().map(|k| (ROW_UNITS / 3.0, Some(*k))).collect())
            .collect(),
    }
}

/// Key rectangles for the current layer, in panel pixels.
pub fn keys(size: (u32, u32), state: &OskState) -> Vec<(Rect, Key)> {
    let w = f64::from(size.0);
    let unit = w / ROW_UNITS;
    let gap = w * GAP;
    let (top, bottom) = extent(size);
    let rows = rows(state);
    // Fewer rows share the same tray, so text keys get taller targets.
    let row_h = f64::from(bottom - top) / rows.len() as f64;
    let mut out = Vec::new();
    for (row_index, row) in rows.into_iter().enumerate() {
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
    const LAYOUTS: [Layout; 3] = [Layout::Text, Layout::Phone, Layout::Terminal];

    fn states(layout: Layout) -> [OskState; 4] {
        let base = OskState::new(layout);
        [
            base,
            OskState {
                shift: true,
                ..base
            },
            OskState {
                symbols: true,
                ..base
            },
            OskState {
                symbols: true,
                shift: true,
                ..base
            },
        ]
    }

    #[test]
    fn layout_stays_inside_the_panel_and_keys_do_not_overlap() {
        for layout in LAYOUTS {
            for state in states(layout) {
                let keys = keys(SIZE, &state);
                // Top row, middle row (9 letters or 10 symbols), the
                // shift/backspace row, and the space row; the terminal adds
                // the accessory row and the dial pad has 5 rows of 3.
                let middle = if state.symbols { 10 } else { 9 };
                let expected = match layout {
                    Layout::Text => 10 + middle + 9 + 5,
                    Layout::Terminal => 10 + 10 + middle + 9 + 5,
                    Layout::Phone => 15,
                };
                assert_eq!(keys.len(), expected, "{state:?}");
                let (top, bottom) = extent(SIZE);
                assert!(keys.iter().any(|(r, _)| r.y + r.h > bottom - 12));
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
    }

    #[test]
    fn layouts_carry_their_own_key_sets() {
        let has = |state: &OskState, key: Key| keys(SIZE, state).iter().any(|(_, k)| *k == key);
        let text = OskState::default();
        assert_eq!(text.layout, Layout::Text, "fields default to text");
        assert!(has(&text, Key::Shift) && has(&text, Key::Layer) && has(&text, Key::Space));
        for terminal_only in [Key::Esc, Key::Tab, Key::Ctrl, Key::Up] {
            assert!(!has(&text, terminal_only), "{terminal_only:?}");
            assert!(has(&OskState::new(Layout::Terminal), terminal_only));
        }
        let phone = OskState::new(Layout::Phone);
        let labels: String = keys(SIZE, &phone)
            .iter()
            .filter_map(|(_, k)| match k {
                Key::Char(c) => Some(*c),
                _ => None,
            })
            .collect();
        assert_eq!(labels, "123456789*0#+");
        assert!(has(&phone, Key::Backspace) && has(&phone, Key::Enter));
        assert!(!has(&phone, Key::Shift) && !has(&phone, Key::Char('a')));
        // Text keys are taller than terminal keys in the same tray.
        let height = |state: &OskState| {
            keys(SIZE, state)
                .into_iter()
                .find(|(_, k)| *k == Key::Char('q'))
                .unwrap()
                .0
                .h
        };
        assert!(height(&text) > height(&OskState::new(Layout::Terminal)));
        // Switching layouts drops sticky modifiers; keeping it does not.
        let shifted = OskState {
            shift: true,
            ..text
        };
        assert_eq!(shifted.with_layout(Layout::Text), shifted);
        assert_eq!(
            shifted.with_layout(Layout::Phone),
            OskState::new(Layout::Phone)
        );
        assert!(
            !OskState {
                shift: true,
                ..phone
            }
            .upper()
        );
    }

    #[test]
    fn every_printable_ascii_character_is_reachable() {
        for layout in [Layout::Text, Layout::Terminal] {
            let mut seen = std::collections::HashSet::new();
            for state in states(layout) {
                for (_, key) in keys(SIZE, &state) {
                    if let Key::Char(c) = key {
                        let c = if state.upper() {
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
                assert!(
                    seen.contains(&(c as char)),
                    "{layout:?} missing {:?}",
                    c as char
                );
            }
        }
    }

    #[test]
    fn shift_is_one_shot_then_caps_lock_then_off() {
        let mut state = OskState::default();
        assert_eq!(state.press(Key::Shift, false), None);
        assert!(state.shift_active() && !state.caps_lock);
        assert_eq!(label(Key::Char('a'), &state), "A");
        assert_eq!(state.press(Key::Char('a'), false), Some(b"A".to_vec()));
        assert_eq!(state.press(Key::Char('b'), false), Some(b"b".to_vec()));
        // Second tap while armed locks capitals.
        state.press(Key::Shift, false);
        state.press(Key::Shift, false);
        assert!(state.caps_lock && state.shift_active());
        assert_eq!(state.press(Key::Char('o'), false), Some(b"O".to_vec()));
        assert_eq!(state.press(Key::Char('k'), false), Some(b"K".to_vec()));
        // Symbols are not cased, and the lock survives the symbol page.
        state.press(Key::Layer, false);
        assert!(!state.shift_active(), "symbol page shift is separate");
        assert_eq!(state.press(Key::Char('1'), false), Some(b"1".to_vec()));
        state.press(Key::Layer, false);
        assert_eq!(state.press(Key::Char('z'), false), Some(b"Z".to_vec()));
        // Third tap releases it.
        state.press(Key::Shift, false);
        assert!(!state.caps_lock && !state.shift_active());
        assert_eq!(state.press(Key::Char('z'), false), Some(b"z".to_vec()));
    }

    #[test]
    fn sticky_modifiers_and_control_bytes() {
        let mut state = OskState::new(Layout::Terminal);
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
