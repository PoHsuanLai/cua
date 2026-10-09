//! What the machine remembers between inputs beyond the phase: small, typed, and never pixels or
//! thought text.

use crate::sensitivity::WindowSensitivity;
use cua_bus::{ActionOutcome, AuthorisedBy, FrameNote, ThoughtDigest};
use cua_desktop::Stamp;
use maskcap::DamageAt;
use serde::{Deserialize, Serialize};

/// How often the capture of this step has been tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTries {
    /// Not failed yet.
    First,
    /// Failed once and tried again; the next failure ends the run.
    Retried,
}

/// What a `Settling` phase is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SettleWatch {
    /// Nothing.
    Idle,
    /// The screen to stop changing after an action that ran at `since`; `latest` is the newest
    /// damage seen.
    Damage {
        /// When the action ran.
        since: Stamp,
        /// The newest change.
        latest: Option<DamageAt>,
    },
    /// A fixed pause (the model's `wait`) that ends at `until`.
    Timer {
        /// When.
        until: Stamp,
    },
}

/// Whether the head action waits for the person's yes because its window is sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Held {
    /// Nothing is held; a question that is waiting is the model's own.
    #[default]
    Nothing,
    /// The head action waits for the person's answer to the question the run asked for it.
    Sensitive(WindowSensitivity),
}

/// The machine's own notes on the current run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTrack {
    /// The capture retry of the current step.
    pub capture: CaptureTries,
    /// How this step's actions ended so far; they become the next observation's `prev`.
    pub outcomes: Vec<ActionOutcome>,
    /// When actions ran in the last minute, for the rate limit.
    pub acted: Vec<Stamp>,
    /// What `Settling` waits for.
    pub settle: SettleWatch,
    /// Who allowed the action now being carried out.
    pub authorised: AuthorisedBy,
    /// The digest of the current step's thought, if the model gave one.
    pub thought: Option<ThoughtDigest>,
    /// What is kept of the current step's frame.
    pub frame: Option<FrameNote>,
    /// How many of the current batch's actions the model already announced while it streamed
    /// (so the reply announces only the rest).
    pub proposed: u32,
    /// Whether the question now waiting was asked by the run for a sensitive window.
    #[serde(default)]
    pub held: Held,
}

impl RunTrack {
    /// A run that has not started a step.
    pub fn new() -> RunTrack {
        RunTrack {
            capture: CaptureTries::First,
            outcomes: Vec::new(),
            acted: Vec::new(),
            settle: SettleWatch::Idle,
            authorised: AuthorisedBy::Policy,
            thought: None,
            frame: None,
            proposed: 0,
            held: Held::Nothing,
        }
    }
}

impl Default for RunTrack {
    fn default() -> Self {
        RunTrack::new()
    }
}
