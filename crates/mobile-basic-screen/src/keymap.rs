//! Minimal evdev keycode to terminal-input mapping for a physical or virtual
//! (QEMU `send-key`) keyboard reaching the guest through `wl_keyboard`.
//!
//! Hand-rolled on purpose: the guest client is a static musl binary, so
//! linking libxkbcommon (which `smithay-client-toolkit`'s `xkbcommon` feature
//! requires) is impractical, and the touch keyboard is the primary phone input.
//! A US layout table covers the QEMU agent workflow and host-window typing.
#![cfg_attr(not(target_os = "linux"), allow(dead_code))]

use crate::osk::{self, Key};

/// Modifier state from `wl_keyboard.modifiers`, interpreted with the default
/// xkb modifier order (Shift, Lock, Control, Mod1/Alt).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub caps_lock: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Modifiers {
    pub fn from_xkb(depressed: u32, locked: u32) -> Self {
        let active = depressed | locked;
        Self {
            shift: active & 0x1 != 0,
            caps_lock: locked & 0x2 != 0,
            ctrl: active & 0x4 != 0,
            alt: active & 0x8 != 0,
        }
    }
}

/// `(evdev code, unshifted, shifted)` for the printable non-letter keys.
const PRINTABLE: &[(u32, char, char)] = &[
    (2, '1', '!'),
    (3, '2', '@'),
    (4, '3', '#'),
    (5, '4', '$'),
    (6, '5', '%'),
    (7, '6', '^'),
    (8, '7', '&'),
    (9, '8', '*'),
    (10, '9', '('),
    (11, '0', ')'),
    (12, '-', '_'),
    (13, '=', '+'),
    (26, '[', '{'),
    (27, ']', '}'),
    (39, ';', ':'),
    (40, '\'', '"'),
    (41, '`', '~'),
    (43, '\\', '|'),
    (51, ',', '<'),
    (52, '.', '>'),
    (53, '/', '?'),
];

const LETTER_ROWS: [(u32, &str); 3] = [(16, "qwertyuiop"), (30, "asdfghjkl"), (44, "zxcvbnm")];

/// Maps an evdev keycode (as delivered by `wl_keyboard.key`) to a key.
/// Returns the character already shifted; letters stay lowercase so the
/// caller can apply shift/caps-lock through [`bytes`].
pub fn key_for(code: u32, shift: bool) -> Option<Key> {
    for (start, letters) in LETTER_ROWS {
        if let Some(offset) = code.checked_sub(start)
            && let Some(letter) = letters.chars().nth(offset as usize)
        {
            return Some(Key::Char(letter));
        }
    }
    if let Some((_, plain, shifted)) = PRINTABLE.iter().find(|(c, _, _)| *c == code) {
        return Some(Key::Char(if shift { *shifted } else { *plain }));
    }
    Some(match code {
        1 => Key::Esc,
        14 => Key::Backspace,
        15 => Key::Tab,
        28 | 96 => Key::Enter,
        57 => Key::Space,
        102 => Key::Home,
        103 => Key::Up,
        104 => Key::PageUp,
        105 => Key::Left,
        106 => Key::Right,
        107 => Key::End,
        108 => Key::Down,
        109 => Key::PageDown,
        111 => Key::Delete,
        _ => return None,
    })
}

/// Bytes to write to the PTY for a key press, or `None` for keys with no
/// terminal meaning (modifiers, function keys).
pub fn bytes(code: u32, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let key = key_for(code, mods.shift)?;
    let upper =
        matches!(key, Key::Char(c) if c.is_ascii_alphabetic()) && (mods.shift ^ mods.caps_lock);
    let mut out = osk::bytes(key, upper, mods.ctrl, app_cursor)?;
    if mods.alt {
        out.insert(0, 0x1b);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_letters_symbols_and_controls() {
        let plain = Modifiers::default();
        assert_eq!(bytes(16, plain, false), Some(b"q".to_vec()));
        assert_eq!(
            bytes(16, Modifiers::from_xkb(0x1, 0), false),
            Some(b"Q".to_vec())
        );
        assert_eq!(
            bytes(16, Modifiers::from_xkb(0, 0x2), false),
            Some(b"Q".to_vec())
        );
        assert_eq!(
            bytes(2, Modifiers::from_xkb(0x1, 0), false),
            Some(b"!".to_vec())
        );
        assert_eq!(
            bytes(46, Modifiers::from_xkb(0x4, 0), false),
            Some(vec![0x03])
        );
        assert_eq!(bytes(28, plain, false), Some(b"\r".to_vec()));
        assert_eq!(bytes(103, plain, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(
            bytes(33, Modifiers::from_xkb(0x8, 0), false),
            Some(b"\x1bf".to_vec())
        );
        assert_eq!(bytes(42, plain, false), None, "shift alone sends nothing");
    }
}
