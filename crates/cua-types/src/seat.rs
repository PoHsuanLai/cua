//! What the seat reports: how text got in, whose seat, the receipt, and why delivery failed.

use cua_desktop::{LeaseId, Stamp};
use porter_core::Count;
use serde::{Deserialize, Serialize};

/// How text reaches the focused field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextPath {
    /// Key codes in the EIS keymap: ASCII on a US layout.
    EiKeymap,
    /// `ei_text.utf8`: any Unicode, committed through the focused text-input. A stock compositor
    /// has it; it targets the person's focus, so it is not offered there (decision S10).
    EiText,
    /// A temporary keymap through `zwp_virtual_keyboard_v1`. Same reservation as `EiText`.
    VirtualKeyboard,
    /// The fork's agent text input: Unicode, delivered only to the leased window.
    AgentTextInput,
}

/// Whose seat the input went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum InputOrigin {
    /// The lease's own agent seat (fork).
    AgentSeat(LeaseId),
    /// EI on the person's seat (stock).
    SharedSeatViaEi,
}

/// A batch of input that was delivered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputReceipt {
    /// The lease.
    pub lease: LeaseId,
    /// How many operations.
    pub ops: Count,
    /// The text path used, if any text was typed.
    pub text_path: Option<TextPath>,
    /// Whose seat.
    pub origin: InputOrigin,
    /// When.
    pub at: Stamp,
}

/// Why the seat could not deliver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum SeatError {
    /// No EIS connection.
    #[error("not connected")]
    NotConnected,
    /// The compositor refused.
    #[error("refused")]
    Refused,
    /// The operation lies outside the lease's window.
    #[error("outside the lease")]
    OutsideLease,
    /// The window is gone.
    #[error("the target is gone")]
    TargetGone,
    /// Emulation is stopped.
    #[error("suspended")]
    Suspended,
    /// The keymap could not be read.
    #[error("keymap: {0}")]
    Keymap(String),
    /// The EIS server's answer did not follow the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_receipt_pins_its_json() {
        let receipt = InputReceipt {
            lease: LeaseId(3),
            ops: Count(5),
            text_path: Some(TextPath::EiKeymap),
            origin: InputOrigin::SharedSeatViaEi,
            at: Stamp(1200),
        };
        let json = serde_json::to_string(&receipt).unwrap();
        assert_eq!(
            json,
            r#"{"lease":3,"ops":5,"text_path":"ei_keymap","origin":{"kind":"shared_seat_via_ei"},"at":1200}"#
        );
        assert_eq!(
            serde_json::from_str::<InputReceipt>(&json).unwrap(),
            receipt
        );
        let fork = InputOrigin::AgentSeat(LeaseId(3));
        assert_eq!(
            serde_json::to_string(&fork).unwrap(),
            r#"{"kind":"agent_seat","v":3}"#
        );
    }
}
