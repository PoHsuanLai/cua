//! The keymap the fake seat announces: every printable ASCII character on its own key, and the
//! named keys the surfaces answer to. A key code is only a number here, so `decode` turns it
//! back into what it stands for.

use crate::surface::Stroke;
use keyboard_types::Key;
use seat_input::{Keycode, Keymap, KeymapEntry, Produces, ShiftLevel};

/// The named keys, each with its code.
const NAMED: &[(Key, u32)] = &[
    (Key::Enter, 1),
    (Key::Tab, 2),
    (Key::Backspace, 3),
    (Key::Escape, 4),
    (Key::Delete, 5),
    (Key::Control, 6),
    (Key::Shift, 7),
    (Key::Alt, 8),
    (Key::Meta, 9),
    (Key::ArrowUp, 10),
    (Key::ArrowDown, 11),
    (Key::ArrowLeft, 12),
    (Key::ArrowRight, 13),
    (Key::Home, 14),
    (Key::End, 15),
    (Key::PageUp, 16),
    (Key::PageDown, 17),
];

/// Where the character codes start.
const CHAR_BASE: u32 = 1000;

/// What a key code stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decoded {
    /// A character.
    Char(char),
    /// A key a surface answers to.
    Stroke(Stroke),
    /// A modifier that makes the next keys a shortcut, which no surface has.
    Command,
    /// Any other key.
    Other,
}

/// The keymap of the fake seat.
pub fn keymap() -> Keymap {
    let named = NAMED.iter().map(|(key, code)| KeymapEntry {
        produces: Produces::Named(key.to_string()),
        code: Keycode(*code),
        level: ShiftLevel::Plain,
    });
    let chars = (' '..='~').map(|c| KeymapEntry {
        produces: Produces::Char(c),
        code: Keycode(CHAR_BASE + u32::from(c)),
        level: ShiftLevel::Plain,
    });
    Keymap {
        entries: named.chain(chars).collect(),
    }
}

/// What `code` stands for.
pub fn decode(code: Keycode) -> Decoded {
    if let Some(c) = code
        .0
        .checked_sub(CHAR_BASE)
        .and_then(char::from_u32)
        .filter(|c| (' '..='~').contains(c))
    {
        return Decoded::Char(c);
    }
    match NAMED.iter().find(|(_, n)| *n == code.0).map(|(k, _)| k) {
        Some(Key::Enter) => Decoded::Stroke(Stroke::Enter),
        Some(Key::Tab) => Decoded::Stroke(Stroke::Tab),
        Some(Key::Backspace) => Decoded::Stroke(Stroke::Backspace),
        Some(Key::Escape) => Decoded::Stroke(Stroke::Escape),
        Some(Key::Control | Key::Alt | Key::Meta) => Decoded::Command,
        _ => Decoded::Other,
    }
}
