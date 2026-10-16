//! What the optional model check is asked and answers.
//!
//! The deterministic check cannot see a paraphrase. When the person has switched the reader on,
//! the screen's text is also handed to the quarantined reader, which reads untrusted text under a
//! closed task, has no tools and answers a typed value only. cua asks for one of three words
//! ([`ScreenRead`]); nothing the reader says becomes text in the run.

use super::excerpt::readable;
use super::{ScreenView, is_secret};
use a11y_tree::NodeValue;
use serde::{Deserialize, Serialize};

/// The most characters of screen text one question to the reader holds.
pub const READER_CHARS: usize = 4_000;

/// Less than this many characters is not worth a question.
const READER_MIN_CHARS: usize = 20;

/// The screen's text, as the reader is asked about it. It is screen content: `Debug` prints its
/// length only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScreenText(String);

impl std::fmt::Debug for ScreenText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ScreenText(<{} chars>)", self.0.chars().count())
    }
}

impl ScreenText {
    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The text of `view`: the title, then each node's name and text value, one per line, cut at
    /// [`READER_CHARS`]. Secret nodes are left out. `None` when there is too little to ask about.
    pub fn of(view: &ScreenView<'_>) -> Option<ScreenText> {
        let nodes = view.nodes.iter().filter(|n| !is_secret(n)).flat_map(|n| {
            let value = match &n.value {
                NodeValue::Text(text) => Some(text.value.as_str()),
                NodeValue::None | NodeValue::Range { .. } => None,
            };
            std::iter::once(n.name.value.as_str()).chain(value)
        });
        let mut text = String::new();
        let mut chars = 0usize;
        for part in std::iter::once(view.title).chain(nodes) {
            let part = readable(part);
            if part.is_empty() {
                continue;
            }
            if !text.is_empty() {
                text.push('\n');
                chars += 1;
            }
            chars += part.chars().count();
            text.push_str(&part);
            if chars >= READER_CHARS {
                break;
            }
        }
        let clipped: String = text.chars().take(READER_CHARS).collect();
        (clipped.chars().count() >= READER_MIN_CHARS).then_some(ScreenText(clipped))
    }
}

/// What the reader made of the screen's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenRead {
    /// Nothing aimed at an assistant.
    Clear,
    /// Text that reads as instructions for an assistant.
    Instructions,
    /// The reader did not answer (not wired, not running, an answer that did not fit): the
    /// deterministic check stands alone.
    Unavailable,
}
