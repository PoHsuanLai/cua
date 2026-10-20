//! Why taking a window failed after the desktop had leased it: which step of the taking stalled
//! or broke, and the plain words the report says for it.

use serde::{Deserialize, Serialize};

/// A step of taking a leased window, after the lease itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcquirePhase {
    /// Connecting the window's input.
    Attach,
    /// Starting to see the window.
    Capture,
}

/// How a step failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AcquireWhy {
    /// It did not answer in time.
    TimedOut,
    /// It answered with an error (technical words, for the log only).
    Failed(String),
}

/// A step of taking a window that failed, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AcquireFailure {
    /// The step.
    pub phase: AcquirePhase,
    /// How it failed.
    pub why: AcquireWhy,
}

impl AcquireFailure {
    /// What the report says: the step is named in plain words.
    pub fn words(&self) -> &'static str {
        match (self.phase, &self.why) {
            (AcquirePhase::Attach, AcquireWhy::TimedOut) => {
                "The window was taken, but its input never answered, so it could not be controlled."
            }
            (AcquirePhase::Attach, AcquireWhy::Failed(_)) => {
                "The window was taken, but its input could not be connected."
            }
            (AcquirePhase::Capture, AcquireWhy::TimedOut) => {
                "The window was taken, but its picture never arrived, so it could not be seen."
            }
            (AcquirePhase::Capture, AcquireWhy::Failed(_)) => {
                "The window was taken, but its picture could not be started."
            }
        }
    }
}
