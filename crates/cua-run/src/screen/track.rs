//! What a run remembers about suspicious screens.

use super::{ScreenKind, WindowKey};
use serde::{Deserialize, Serialize};

/// Whether the screen's text is also handed to the quarantined reader.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderMode {
    /// No: the deterministic check stands alone.
    #[default]
    Off,
    /// Yes, when the deterministic check finds nothing.
    On,
}

/// Whether the quarantined reader has been asked about the current look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderWait {
    /// No question is out.
    #[default]
    Idle,
    /// The look waits for the reader's answer.
    Reading,
}

/// The finding the person is being asked about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asking {
    /// The window it was in.
    pub window: WindowKey,
    /// What was found.
    pub kinds: Vec<ScreenKind>,
}

/// The screen check's notes on a run.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ScreenTrack {
    /// Windows and kinds the person said to go on with: asked once per run.
    pub allowed: Vec<(WindowKey, ScreenKind)>,
    /// Whether the run has seen suspicious content, whatever the person answered. From then on
    /// a step that sends something out or removes something asks.
    pub suspicious: bool,
    /// The finding now being asked about.
    pub asking: Option<Asking>,
    /// The reader, if it was asked.
    pub reader: ReaderWait,
}
