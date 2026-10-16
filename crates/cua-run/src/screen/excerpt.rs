//! Screen text as the check reads it, and the bounded piece of it a reason may carry.

use super::hidden::is_invisible;
use serde::{Deserialize, Serialize};

/// The most characters an excerpt holds.
pub const EXCERPT_CHARS: usize = 80;

/// How many characters of context an excerpt keeps before the match.
const LEAD_CHARS: usize = 16;

/// A short piece of screen text, kept so the person can be shown what was found. It is screen
/// content: whoever shows it labels it `Untrusted(Screen)` and shows it as data. It is never
/// longer than [`EXCERPT_CHARS`] (plus a mark at each cut end), holds no control or invisible
/// character, and is never written to the event log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Excerpt(String);

impl Excerpt {
    /// The excerpt of `shown` (text already made [`readable`]) around the character at `at`.
    pub(super) fn around(shown: &str, at: usize) -> Excerpt {
        let chars: Vec<char> = shown.chars().collect();
        let start = at.saturating_sub(LEAD_CHARS).min(chars.len());
        let end = start.saturating_add(EXCERPT_CHARS).min(chars.len());
        let mut text = String::new();
        if start > 0 {
            text.push('\u{2026}');
        }
        text.extend(chars[start..end].iter());
        if end < chars.len() {
            text.push('\u{2026}');
        }
        Excerpt(text)
    }

    /// The excerpt of the start of `shown`.
    pub(super) fn start_of(shown: &str) -> Excerpt {
        Excerpt::around(shown, 0)
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The text a person would read: tag characters decoded to the ASCII they spell, every other
/// invisible and control character dropped, curly quotes straightened, runs of white space
/// collapsed to one space. Case is kept.
pub(super) fn readable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut gap = false;
    for c in text.chars() {
        let c = match c {
            '\u{E0020}'..='\u{E007E}' => char::from_u32(u32::from(c) - 0xE0000).unwrap_or(' '),
            '\u{2018}' | '\u{2019}' => '\'',
            '\u{201C}' | '\u{201D}' => '"',
            other => other,
        };
        if c.is_whitespace() {
            gap = true;
        } else if !is_invisible(c) && !c.is_control() {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(c);
        }
    }
    out
}

/// `shown` in lower case, one character for one character, so a position in the folded text is
/// the same position in `shown`.
pub(super) fn fold(shown: &str) -> String {
    shown
        .chars()
        .map(|c| c.to_lowercase().next().unwrap_or(c))
        .collect()
}
