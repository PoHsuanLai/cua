//! The phrase check: text that reads as instructions aimed at an assistant.
//!
//! The phrases are data ([`InjectionRules`], from `dist/screen-rules.toml`), grouped by the style
//! of attack ([`Family`]). To cover a new phrase, add it to its group in that file; to cover a new
//! style, add a [`Family`] and a group. The text is folded before it is looked for, so case,
//! spacing and zero-width characters between letters do not matter.
//!
//! The trade-off is false positives against misses. A phrase that is only a threat when someone
//! speaks to an assistant ("you are now", "act as") is matched only when the same text also names
//! one (`addressees`); a role marker ("system:", "assistant:") only counts at the start of a
//! text, and only when what follows is addressed or a strong directive (`directives`). So a
//! code editor that shows the word "system", a chat that lists "Assistant" as a contact and a
//! status line reading "System: ready" stay clear. A paraphrase the table lacks is a miss
//! (FINDINGS.md says so).

use crate::rules::{InjectionRules, Match, PhraseGroup};
use serde::{Deserialize, Serialize};

/// The style of attack a phrase belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// "Ignore your previous instructions."
    IgnorePrevious,
    /// A block that claims to be an important message from the person.
    ImportantMessage,
    /// Text dressed as the system, a policy update or a chat template.
    FakeSystem,
    /// A chat role marker ("assistant:", "system:") that opens a text.
    RoleMarker,
    /// A tool or function call written out for the assistant to make.
    ToolCall,
    /// Text that speaks to an assistant, a model or an AI agent by name.
    AddresseeCue,
    /// A link that carries a password, token or key in its address.
    CredentialLink,
}

/// One phrase found: its family, and where (in characters from the start of the folded text).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Hit {
    pub family: Family,
    pub at: usize,
}

fn is_word_char(c: Option<char>) -> bool {
    c.is_some_and(char::is_alphanumeric)
}

/// Where `phrase` first stands in `folded` on whole-word edges (an edge that is punctuation in
/// the phrase needs no word edge), as a byte offset.
fn find_phrase(folded: &str, phrase: &str) -> Option<usize> {
    let first = phrase.chars().next();
    let last = phrase.chars().next_back();
    folded.match_indices(phrase).map(|(at, _)| at).find(|&at| {
        let before = folded[..at].chars().next_back();
        let after = folded[at + phrase.len()..].chars().next();
        let front = !is_word_char(first) || !is_word_char(before);
        let back = !is_word_char(last) || !is_word_char(after);
        front && back
    })
}

/// Whether the folded text names an assistant.
fn names_addressee(rules: &InjectionRules, folded: &str) -> bool {
    folded.contains("language model")
        || folded
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| rules.addressees.iter().any(|a| a == word))
}

/// Whether `rest` holds a word of the `directives`, or names an assistant.
fn orders_or_names(rules: &InjectionRules, rest: &str) -> bool {
    names_addressee(rules, rest)
        || rest
            .split(|c: char| !c.is_alphanumeric())
            .any(|word| rules.directives.iter().any(|d| d == word))
}

/// Whether the match at byte `at` opens a text or a sentence.
fn opens_sentence(folded: &str, at: usize) -> bool {
    let mut before = folded[..at].chars().rev();
    match before.next() {
        None => true,
        Some(' ') => before.next().is_some_and(|c| ".!?:;)]>".contains(c)),
        Some(_) => false,
    }
}

/// The first place a phrase of `group` is found in `folded`.
fn in_group(
    rules: &InjectionRules,
    group: &PhraseGroup,
    folded: &str,
    addressed: bool,
) -> Option<usize> {
    group.phrases.iter().find_map(|phrase| match group.how {
        Match::Anywhere => find_phrase(folded, phrase),
        Match::Addressed => find_phrase(folded, phrase).filter(|_| addressed),
        Match::Opening => folded.match_indices(phrase).map(|(at, _)| at).find(|&at| {
            opens_sentence(folded, at) && orders_or_names(rules, &folded[at + phrase.len()..])
        }),
    })
}

fn char_index(folded: &str, byte: usize) -> usize {
    folded[..byte].chars().count()
}

/// Every family with a phrase in `folded` (text already folded), once each, with the place of
/// its first phrase.
pub(super) fn scan(rules: &InjectionRules, folded: &str) -> Vec<Hit> {
    let addressed = names_addressee(rules, folded);
    let mut hits: Vec<Hit> = Vec::new();
    for group in &rules.groups {
        if hits.iter().any(|h| h.family == group.family) {
            continue;
        }
        if let Some(byte) = in_group(rules, group, folded, addressed) {
            hits.push(Hit {
                family: group.family,
                at: char_index(folded, byte),
            });
        }
    }
    for together in &rules.together {
        if hits.iter().any(|h| h.family == together.family) {
            continue;
        }
        let places: Option<Vec<usize>> = together
            .fragments
            .iter()
            .map(|fragment| folded.find(fragment))
            .collect();
        if let Some(first) = places.and_then(|p| p.into_iter().min()) {
            hits.push(Hit {
                family: together.family,
                at: char_index(folded, first),
            });
        }
    }
    hits
}
