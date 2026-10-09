//! Minimal evdev keycode to terminal-input mapping for a physical or virtual
//! (QEMU `send-key`) keyboard reaching the guest through `wl_keyboard`.
//!
//! Hand-rolled on purpose: the guest client is a static musl binary, so
//! linking libxkbcommon (which `smithay-client-toolkit`'s `xkbcommon` feature
//! requires) is impractical, and the touch keyboard is the primary phone input.
//! A US layout table covers the QEMU agent workflow and host-window typing.
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
/// Returns the character already shifted; letters stay lowercase. Text entry
/// should use [`text_key`] (or [`char_for`]), which also applies Shift and
/// Caps Lock to letters; the terminal path uses [`bytes`].
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

/// [`key_for`] with letter case applied: Shift xor Caps Lock capitalises
/// letters; Caps Lock leaves digits and punctuation alone (US layout).
fn cased_key(code: u32, mods: Modifiers) -> Option<Key> {
    Some(match key_for(code, mods.shift)? {
        Key::Char(c) if c.is_ascii_alphabetic() && (mods.shift ^ mods.caps_lock) => {
            Key::Char(c.to_ascii_uppercase())
        }
        key => key,
    })
}

/// The printable character a key press types with the given modifiers on a
/// US layout (letters, digits, punctuation, space), or `None` for
/// non-printing keys and Ctrl/Alt chords, which are shortcuts, not text.
pub fn char_for(code: u32, mods: Modifiers) -> Option<char> {
    match text_key(code, mods)? {
        Key::Char(c) => Some(c),
        Key::Space => Some(' '),
        _ => None,
    }
}

/// The key a hardware press means for a text field: characters carry their
/// final case and shifted symbol (see [`char_for`]); editing and navigation
/// keys pass through unchanged. Ctrl/Alt chords on printable keys return
/// `None` so a shortcut never inserts a stray letter.
pub fn text_key(code: u32, mods: Modifiers) -> Option<Key> {
    let key = cased_key(code, mods)?;
    if (mods.ctrl || mods.alt) && matches!(key, Key::Char(_) | Key::Space) {
        return None;
    }
    Some(key)
}

/// Bytes to write to the PTY for a key press, or `None` for keys with no
/// terminal meaning (modifiers, function keys).
pub fn bytes(code: u32, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let key = cased_key(code, mods)?;
    let mut out = osk::bytes(key, false, mods.ctrl, app_cursor)?;
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
        let caps_ctrl = Modifiers::from_xkb(0x4, 0x2);
        assert_eq!(bytes(46, caps_ctrl, false), Some(vec![0x03]));
    }

    const SHIFT: Modifiers = Modifiers {
        shift: true,
        caps_lock: false,
        ctrl: false,
        alt: false,
    };
    const CAPS: Modifiers = Modifiers {
        shift: false,
        caps_lock: true,
        ctrl: false,
        alt: false,
    };

    fn typed(codes: &[(u32, Modifiers)]) -> String {
        codes.iter().filter_map(|&(c, m)| char_for(c, m)).collect()
    }

    #[test]
    fn char_for_applies_shift_and_caps_lock() {
        let plain = Modifiers::default();
        let both = Modifiers {
            shift: true,
            caps_lock: true,
            ..plain
        };
        // Letters: shift xor caps lock.
        assert_eq!(char_for(30, plain), Some('a'));
        assert_eq!(char_for(30, SHIFT), Some('A'));
        assert_eq!(char_for(30, CAPS), Some('A'));
        assert_eq!(char_for(30, both), Some('a'));
        // Shifted US symbols; caps lock does not shift them.
        for (code, plain_c, shifted_c) in PRINTABLE {
            assert_eq!(char_for(*code, plain), Some(*plain_c));
            assert_eq!(char_for(*code, SHIFT), Some(*shifted_c));
            assert_eq!(char_for(*code, CAPS), Some(*plain_c));
        }
        assert_eq!(char_for(57, SHIFT), Some(' '));
        // Non-printing keys and chords type nothing.
        assert_eq!(char_for(14, plain), None);
        assert_eq!(char_for(28, plain), None);
        assert_eq!(char_for(42, plain), None);
        assert_eq!(char_for(30, Modifiers::from_xkb(0x4, 0)), None);
        assert_eq!(char_for(30, Modifiers::from_xkb(0x8, 0)), None);
        // "Audit Person" typed with a held shift, and a mixed passphrase.
        assert_eq!(
            typed(&[
                (30, SHIFT),
                (22, plain),
                (32, plain),
                (23, plain),
                (20, plain),
                (57, plain),
                (25, SHIFT),
                (18, plain),
                (19, plain),
                (31, plain),
                (24, plain),
                (49, plain),
            ]),
            "Audit Person"
        );
        assert_eq!(
            typed(&[(25, CAPS), (30, CAPS), (31, plain), (2, SHIFT), (3, plain)]),
            "PAs!2"
        );
    }

    #[test]
    fn text_key_keeps_editing_keys() {
        let plain = Modifiers::default();
        assert_eq!(text_key(16, SHIFT), Some(Key::Char('Q')));
        assert_eq!(text_key(4, SHIFT), Some(Key::Char('#')));
        assert_eq!(text_key(14, SHIFT), Some(Key::Backspace));
        assert_eq!(text_key(28, plain), Some(Key::Enter));
        assert_eq!(text_key(1, plain), Some(Key::Esc));
        assert_eq!(text_key(57, plain), Some(Key::Space));
        assert_eq!(
            text_key(14, Modifiers::from_xkb(0x4, 0)),
            Some(Key::Backspace)
        );
    }
}
