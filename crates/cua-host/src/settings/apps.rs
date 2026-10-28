//! `cua.apps`: which apps computer use may drive and how it asks for each.

use docket_permissions_core::Level;
use porter_core::AppName;

/// How computer use is allowed for one app (design/22 section 3.29, QUESTIONS S9). It is the
/// default the person set up: the start question of a conversation's first run in the app answers
/// for that conversation (the router keeps it), and the mode switch changes the rest in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppAllow {
    /// Ask at a conversation's first run in the app; the answer holds for the conversation.
    Ask,
    /// Never ask. Never for a browser.
    Always,
}

impl AppAllow {
    /// The words of the schema's `allow` column, in order.
    pub const WORDS: [&'static str; 2] = ["ask", "always"];

    /// The allow a word names. `session`, a word of an earlier schema, reads as `ask`: a
    /// conversation is what it meant, and the router now remembers the answer for one.
    pub fn of_word(word: &str) -> Option<Self> {
        match word {
            "ask" | "session" => Some(AppAllow::Ask),
            "always" => Some(AppAllow::Always),
            _ => None,
        }
    }
}

impl AppAllow {
    /// The consent level the router is asked for under this allow: `Ask` is asked (and the
    /// router keeps the answer for the conversation), `Always` is remembered for the Space.
    pub fn grant_level(self) -> Level {
        match self {
            AppAllow::Ask => Level::AlwaysAsk,
            AppAllow::Always => Level::AlwaysAllow,
        }
    }
}

/// One row of `cua.apps`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AppRule {
    /// The app, by desktop-entry id.
    pub app: AppName,
    /// How it is allowed.
    pub allow: AppAllow,
}

/// Which apps computer use may drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Apps {
    /// Every app: what a supervisor built in code runs under, so its own tests need no list.
    Unrestricted,
    /// Only these (the settings file's `cua.apps`; empty means no app is on).
    Listed(Vec<AppRule>),
}

impl Apps {
    /// The rule for `app`: none when computer use may not drive it.
    pub fn rule_for(&self, app: &AppName) -> Option<AppAllow> {
        match self {
            Apps::Unrestricted => Some(AppAllow::Ask),
            Apps::Listed(rules) => rules.iter().find(|r| &r.app == app).map(|r| r.allow),
        }
    }
}

/// Fragments of an app id that are web browsers (lower case): a browser holds the person's
/// sessions everywhere, so it never takes `always` (QUESTIONS S9).
const BROWSERS: &[&str] = &[
    "firefox",
    "chrom",
    "brave",
    "vivaldi",
    "opera",
    "epiphany",
    "falkon",
    "librewolf",
    "browser",
    "microsoft-edge",
    "msedge",
];

/// Whether `app` is a web browser, as far as its id says.
pub fn is_browser(app: &AppName) -> bool {
    let id = app.as_str().to_lowercase();
    BROWSERS.iter().any(|fragment| id.contains(fragment))
}
