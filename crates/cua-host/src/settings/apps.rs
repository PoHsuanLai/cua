//! `cua.apps`: which apps computer use may drive and how it asks for each.

use docket_core::Level;
use porter_core::AppName;

/// How computer use is allowed for one app (design/22 section 3.29, QUESTIONS S9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AppAllow {
    /// Ask every run.
    Ask,
    /// Ask once a session.
    Session,
    /// Never ask again. Never for a browser.
    Always,
}

impl AppAllow {
    /// The words of the schema's `allow` column, in order.
    pub const WORDS: [&'static str; 3] = ["ask", "session", "always"];

    /// The allow a word names.
    pub fn of_word(word: &str) -> Option<Self> {
        match word {
            "ask" => Some(AppAllow::Ask),
            "session" => Some(AppAllow::Session),
            "always" => Some(AppAllow::Always),
            _ => None,
        }
    }
}

impl AppAllow {
    /// The consent level the router is asked for under this allow.
    ///
    /// `Session` maps to `AlwaysAllow` because docket's level has no per-session form: the
    /// router's own store and its own question cover the rest, and it can only be stricter.
    pub fn grant_level(self) -> Level {
        match self {
            AppAllow::Ask => Level::AlwaysAsk,
            AppAllow::Session | AppAllow::Always => Level::AlwaysAllow,
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
