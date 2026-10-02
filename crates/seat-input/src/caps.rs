//! What a seat can do, which differs by backend.

use cua_desktop::SeatIsolation;
use serde::{Deserialize, Serialize};

/// How text reaches the focused field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextPath {
    /// Key codes in the EIS keymap: ASCII on a US layout.
    EiKeymap,
    /// `ei_text.utf8`: any Unicode, committed through the focused text-input. Stock cosmic-comp
    /// has it; it targets the person's focus, so it is not offered there (decision S10).
    EiText,
    /// A temporary keymap through `zwp_virtual_keyboard_v1`. Same reservation as `EiText`.
    VirtualKeyboard,
    /// The fork's agent text input: Unicode, delivered only to the leased window.
    AgentTextInput,
}

/// How the wheel is delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollKind {
    /// Wheel detents: one detent arrives as one detent (measured on stock).
    Discrete,
    /// Smooth distance.
    Smooth,
}

/// What a seat offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeatCaps {
    /// Whether the agent has a seat of its own.
    pub isolation: SeatIsolation,
    /// The text paths it may use, best first.
    pub text: Vec<TextPath>,
    /// How the wheel is delivered.
    pub scroll: ScrollKind,
}

impl SeatCaps {
    /// What a seat of this isolation offers. Stock types ASCII through key codes only (S10);
    /// the fork's agent seat adds its own text input.
    pub fn for_isolation(isolation: SeatIsolation) -> SeatCaps {
        let text = match isolation {
            SeatIsolation::AgentSeat => vec![TextPath::AgentTextInput, TextPath::EiKeymap],
            SeatIsolation::SharedSeat => vec![TextPath::EiKeymap],
        };
        SeatCaps {
            isolation,
            text,
            scroll: ScrollKind::Discrete,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_offers_no_unicode_text_path_and_the_fork_offers_its_own() {
        let stock = SeatCaps::for_isolation(SeatIsolation::SharedSeat);
        assert_eq!(stock.text, vec![TextPath::EiKeymap]);
        let fork = SeatCaps::for_isolation(SeatIsolation::AgentSeat);
        assert_eq!(fork.text.first(), Some(&TextPath::AgentTextInput));
        for caps in [stock, fork] {
            assert!(!caps.text.contains(&TextPath::EiText));
            assert!(!caps.text.contains(&TextPath::VirtualKeyboard));
            assert_eq!(caps.scroll, ScrollKind::Discrete);
        }
    }
}
