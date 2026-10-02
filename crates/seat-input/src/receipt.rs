//! What was injected, for the event log and the confirmation guard.

use crate::caps::TextPath;
use cua_desktop::{LeaseId, Stamp};
use porter_core::Count;
use serde::{Deserialize, Serialize};

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
