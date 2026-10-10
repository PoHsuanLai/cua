//! An action aimed at the leased window, and how it ended.

use crate::ids::WindowRef;
use cua_action::{CuaAction, WindowSpace};
use cua_types::SeatError;
use docket_core::CallRefusal;
use prov::Labelled;
use serde::{Deserialize, Serialize};

/// An action and the window it is aimed at. Points are in `WindowSpace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetedAction {
    /// The window, by the run's handle.
    pub window: WindowRef,
    /// The action.
    pub action: CuaAction<WindowSpace>,
}

/// What stopped an action before the router saw it: cua's own pre-checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalRefusal {
    /// The action lies outside the leased window.
    OutsideLease,
    /// The action would type into a secret field.
    SecretField,
}

/// Who refused an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Refusal {
    /// The router (policy, consent, breaker, budget).
    Gate(CallRefusal),
    /// cua's own pre-check.
    Local(LocalRefusal),
}

/// How one action ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ActionResult {
    /// Performed.
    Done,
    /// Dropped without running (the rest of a batch after a refusal or a failure).
    NotExecuted,
    /// Refused.
    Refused(Refusal),
    /// The person declined the confirmation.
    UserDeclined,
    /// Input could not be delivered.
    Failed(SeatError),
    /// The person answered the model's question; the text is the person's own words.
    Answered(Labelled<String>),
    /// The person acted in the window since this instant (after a take-over).
    UserActed {
        /// When they took over.
        since: cua_desktop::Stamp,
    },
    /// The success check failed.
    CheckFailed,
}

/// An action and how it ended: the next step's `prev`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionOutcome {
    /// The action.
    pub action: TargetedAction,
    /// How it ended.
    pub result: ActionResult,
}
