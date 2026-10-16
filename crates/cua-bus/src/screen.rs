//! What the screen check found, in the words the bus and the event log keep.
//!
//! The check itself is `cua_run`'s: it reads what the model is about to be shown and says
//! whether it holds text that looks aimed at the assistant. These are the closed values a record
//! and an outcome carry; no screen text is among them.

use serde::{Deserialize, Serialize};

/// What made a screen suspicious. The slug is the serde form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ScreenKind {
    /// Text that reads as instructions for an assistant: an override, a claim to speak for the
    /// person or the system, a tool call, a link that carries credentials.
    Instruction,
    /// Characters the person cannot see: zero-width, bidi overrides, Unicode tag characters.
    HiddenCharacters,
    /// Text the accessibility tree carries and the person cannot see: a node with no size, one
    /// outside the window, one marked as not shown.
    UnseenText,
    /// The quarantined reader, asked about an excerpt, called it instructions for an assistant.
    ReaderFlagged,
}

/// Whether the person was asked about a suspicious screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenAsk {
    /// The run paused and asked.
    Asked,
    /// The person already said to go on for this window and this kind in this run: not asked
    /// again.
    Remembered,
}
