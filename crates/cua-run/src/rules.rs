//! The screen rules: the phrases, window classes, commit words and sensitive titles the run
//! checks a screen against, as plain data. cua-run reads no file: the host reads the shipped
//! `dist/screen-rules.toml` (or the administrator's replacement) and hands the parsed [`Rules`]
//! in, through the seams ([`crate::Parts`]) and the run's environment ([`crate::RunEnv`]).

use crate::screen::Family;
use cua_action::WindowClass;
use serde::{Deserialize, Serialize};

/// Every table the screen check and the window classes read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rules {
    /// Text that reads as instructions aimed at an assistant.
    pub injection: InjectionRules,
    /// What kind of window a step acts in, and how sensitive it is.
    pub windows: WindowRules,
    /// The controls that commit a send, a submission, a payment or a transfer.
    pub commit: CommitRules,
}

/// How a group's phrases are looked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Match {
    /// The phrase anywhere.
    Anywhere,
    /// The phrase anywhere, and the text names an assistant.
    Addressed,
    /// The phrase opens the text (or a sentence of it), and what follows names an assistant or
    /// holds a strong directive.
    Opening,
}

/// Phrases of one family, looked for one way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhraseGroup {
    /// The style of attack.
    pub family: Family,
    /// How the phrases are looked for.
    pub how: Match,
    /// The phrases: lower case, single spaces, straight quotes.
    pub phrases: Vec<String>,
}

/// Fragments that all have to be present: the written-out shape of a tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Together {
    /// The style of attack.
    pub family: Family,
    /// The fragments.
    pub fragments: Vec<String>,
}

/// The injection phrase tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InjectionRules {
    /// Words that name the reader of an injected text. Whole words only.
    pub addressees: Vec<String>,
    /// Verbs strong enough that "system: <verb> ..." reads as an order, not a status line.
    pub directives: Vec<String>,
    /// The phrase groups, in the order they are tried.
    pub groups: Vec<PhraseGroup>,
    /// The written-out shapes.
    pub together: Vec<Together>,
}

/// Fragments that put a window in a class.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassFragments {
    /// The class.
    pub class: WindowClass,
    /// Lower-case fragments, any of which puts the window in the class.
    pub fragments: Vec<String>,
}

/// The window tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowRules {
    /// Fragments of an app id that name a kind of window; the first class that matches wins.
    pub apps: Vec<ClassFragments>,
    /// Fragments of a window title that say money is being paid or moved. A false positive only
    /// costs a question.
    pub money_titles: Vec<ClassFragments>,
    /// Words in a mail window's title that say it is composing.
    pub compose_words: Vec<String>,
    /// Fragments of an app id that are mail clients.
    pub mail_apps: Vec<String>,
    /// Fragments of a window title that say a password or another secret is asked for.
    pub credential_titles: Vec<String>,
    /// Fragments of a window title that say a privilege is being asked for.
    pub admin_titles: Vec<String>,
}

/// The words of a control's name that commit something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitRules {
    /// Words that pay.
    pub pay: Vec<String>,
    /// Words that send, submit, transfer or confirm.
    pub also: Vec<String>,
}

/// The shipped rules, for the unit tests of the crates that read them.
#[cfg(test)]
pub(crate) fn shipped() -> Rules {
    toml::from_str(include_str!("../../../dist/screen-rules.toml"))
        .expect("the shipped screen rules read")
}
