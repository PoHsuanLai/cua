//! The keymap the EIS server sent, reduced to what planning needs.

use crate::op::Keycode;
use serde::{Deserialize, Serialize};

/// Whether a key produces its character with Shift held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShiftLevel {
    /// Without a modifier.
    Plain,
    /// With Shift.
    Shifted,
}

/// What a key produces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Produces {
    /// A character.
    Char(char),
    /// A named key (`Enter`, `Tab`, `Control`, `F5`...), by its W3C `key` value.
    Named(String),
}

/// One row of a keymap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeymapEntry {
    /// What it produces.
    pub produces: Produces,
    /// The key.
    pub code: Keycode,
    /// At which level.
    pub level: ShiftLevel,
}

/// An xkb keymap, parsed once when the seat connects.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Keymap {
    /// The rows, in the keymap's own order.
    pub entries: Vec<KeymapEntry>,
}

/// Why a keymap could not be read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum KeymapError {
    /// The text is not an xkb keymap.
    #[error("not an xkb keymap: {0}")]
    Malformed(String),
}

impl Keymap {
    /// Reads the keymap text an EIS server sends on its keyboard device.
    pub fn parse_xkb(text: &str) -> Result<Keymap, KeymapError> {
        let _ = text;
        todo!("read the key names, levels and modifier maps of the xkb text")
    }

    /// The key that types `c`, and whether Shift is needed.
    pub fn stroke_for(&self, c: char) -> Option<(Keycode, ShiftLevel)> {
        self.entries
            .iter()
            .find(|e| e.produces == Produces::Char(c))
            .map(|e| (e.code, e.level))
    }

    /// The key code of a named key or a character key by its W3C `key` value.
    pub fn code_of(&self, key: &keyboard_types::Key) -> Option<Keycode> {
        use keyboard_types::Key;
        let wanted = match key {
            Key::Character(text) => {
                let mut chars = text.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => Produces::Char(c),
                    _ => return None,
                }
            }
            named => Produces::Named(named.to_string()),
        };
        // A shifted letter names the same key as its lower case.
        let lower = match &wanted {
            Produces::Char(c) => Some(Produces::Char(c.to_ascii_lowercase())),
            Produces::Named(_) => None,
        };
        let find = |p: &Produces| {
            self.entries
                .iter()
                .find(|e| &e.produces == p && e.level == ShiftLevel::Plain)
                .map(|e| e.code)
        };
        find(&wanted).or_else(|| lower.as_ref().and_then(find))
    }
}
