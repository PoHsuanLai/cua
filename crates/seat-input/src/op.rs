//! What the seat is asked to do, one step at a time.

use cua_action::{Button, Notches, Point, ScrollDir, TypedText, WindowSpace};
use cua_desktop::Ms;
use serde::{Deserialize, Serialize};

/// A key or button going down or up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Press {
    /// Down.
    Down,
    /// Up.
    Up,
}

/// An evdev key code in the EIS keymap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Keycode(pub u32);

/// One input operation. Points are window-local; the seat maps them to the EI region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum InputOp {
    /// Moves the pointer.
    MoveTo(Point<WindowSpace>),
    /// A pointer button.
    Button {
        /// Which.
        button: Button,
        /// Down or up.
        dir: Press,
    },
    /// A key, by evdev code.
    Key {
        /// Which.
        key: Keycode,
        /// Down or up.
        dir: Press,
    },
    /// Text through a text path that is not key codes (`TextPath::AgentTextInput`).
    Text(TypedText),
    /// Wheel detents.
    Scroll {
        /// Which way.
        dir: ScrollDir,
        /// How many detents.
        notches: Notches,
    },
    /// A pause between steps (press and release, double-click spacing).
    Gap(Ms),
}
