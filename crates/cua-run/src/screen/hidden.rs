//! Characters the person cannot see, and what they hide.
//!
//! Three kinds are told apart. Bidi overrides display text reversed, so what is shown is not what
//! is read. Unicode tag characters (and variation selectors in the supplement block) are
//! invisible and spell ASCII. Zero-width characters sit between the letters of a phrase so it
//! escapes a plain search. Characters that have honest uses (the joiners of emoji and of Persian
//! or Indic scripts, the tag run of a subdivision flag) are not flagged on their own.

use serde::{Deserialize, Serialize};

/// What the invisible characters of a text do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HiddenKind {
    /// A left-to-right or right-to-left override: the text shows reversed or reordered.
    BidiOverride,
    /// Unicode tag characters: invisible, and they spell ASCII.
    Tags,
    /// Zero-width characters between letters.
    ZeroWidth,
}

/// How many zero-width characters (or joiners between ASCII letters) a text may hold before it
/// is called hidden text. One is a soft word break; two are rarely an accident.
const ZERO_WIDTH_MIN: usize = 2;

/// The black flag that opens a subdivision-flag emoji sequence, whose tag run is honest.
const BLACK_FLAG: char = '\u{1F3F4}';

fn is_override(c: char) -> bool {
    matches!(c, '\u{202D}' | '\u{202E}')
}

fn is_tag(c: char) -> bool {
    matches!(c, '\u{E0000}'..='\u{E007F}' | '\u{E0100}'..='\u{E01EF}')
}

/// Zero-width space, word joiner, byte order mark and the Mongolian vowel separator: no honest
/// use inside a word.
fn is_zero_width(c: char) -> bool {
    matches!(c, '\u{200B}' | '\u{2060}' | '\u{FEFF}' | '\u{180E}')
}

fn is_joiner(c: char) -> bool {
    matches!(c, '\u{200C}' | '\u{200D}')
}

/// Whether `c` takes no room on screen and is dropped before the text is read.
pub(super) fn is_invisible(c: char) -> bool {
    is_zero_width(c)
        || is_joiner(c)
        || is_tag(c)
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{200E}'
                | '\u{200F}'
                | '\u{202A}'..='\u{202E}'
                | '\u{2061}'..='\u{2064}'
                | '\u{2066}'..='\u{2069}'
        )
}

/// Whether the tag character at `at` belongs to a subdivision-flag sequence: the run it sits in
/// starts right after a black flag.
fn in_flag_sequence(chars: &[char], at: usize) -> bool {
    let run_start = chars[..at]
        .iter()
        .rposition(|c| !is_tag(*c))
        .map_or(0, |i| i + 1);
    run_start > 0 && chars[run_start - 1] == BLACK_FLAG
}

fn has_tags(chars: &[char]) -> bool {
    chars
        .iter()
        .enumerate()
        .any(|(at, c)| is_tag(*c) && !in_flag_sequence(chars, at))
}

/// Zero-width characters, plus joiners that sit between two ASCII letters (where no script
/// needs them).
fn zero_width_count(chars: &[char]) -> usize {
    let ascii_letter = |c: Option<&char>| c.is_some_and(|c| c.is_ascii_alphabetic());
    chars
        .iter()
        .enumerate()
        .filter(|(at, c)| {
            is_zero_width(**c)
                || (is_joiner(**c)
                    && ascii_letter(at.checked_sub(1).and_then(|i| chars.get(i)))
                    && ascii_letter(chars.get(at + 1)))
        })
        .count()
}

/// The kinds of hidden characters `text` holds, each once, in a fixed order.
pub(super) fn kinds(text: &str) -> Vec<HiddenKind> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = Vec::new();
    if chars.iter().any(|c| is_override(*c)) {
        found.push(HiddenKind::BidiOverride);
    }
    if has_tags(&chars) {
        found.push(HiddenKind::Tags);
    }
    if zero_width_count(&chars) >= ZERO_WIDTH_MIN {
        found.push(HiddenKind::ZeroWidth);
    }
    found
}
