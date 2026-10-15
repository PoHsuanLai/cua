//! The lenient reader (design/22 section 2): a bad value falls back to the base value for that
//! key alone, never fails the file, and is reported so the daemon can log it.

use super::CuaSettings;
use super::apps::{AppAllow, AppRule, Apps, is_browser};
use super::keys::{APPS_KEY, Rule, table};
use porter_core::AppName;
use std::collections::BTreeSet;
use std::fmt;

/// Why one key, one row or the whole file was not used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Why {
    /// The text is not TOML; every key keeps its base value.
    NotToml(String),
    /// The value is not a whole number.
    NotANumber,
    /// The number is outside the key's range.
    OutOfRange {
        /// The smallest the key takes.
        min: i64,
        /// The largest the key takes.
        max: i64,
    },
    /// The value is not one of the key's words.
    NotAWord {
        /// The words the key takes.
        words: &'static [&'static str],
    },
    /// `cua.apps` is not a list of tables.
    NotAList,
    /// A row of `cua.apps` has no usable `app` (a desktop-entry id).
    NotAnApp,
    /// A row of `cua.apps` has an `allow` that is not `ask`, `session` or `always`.
    NotAnAllow,
    /// A browser asked for `always`; it is asked every run instead.
    BrowserNeverAlways,
    /// The file is there and could not be read (permissions, a sandbox that hides the home
    /// directory); the next directory is tried.
    Unreadable(String),
}

impl fmt::Display for Why {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Why::NotToml(why) => write!(f, "not TOML: {why}"),
            Why::NotANumber => write!(f, "not a whole number"),
            Why::OutOfRange { min, max } => write!(f, "outside {min}..={max}"),
            Why::NotAWord { words } => write!(f, "not one of {}", words.join(", ")),
            Why::NotAList => write!(f, "not a list of {{ app, allow }} tables"),
            Why::NotAnApp => write!(f, "no desktop-entry id in `app`"),
            Why::NotAnAllow => write!(f, "`allow` is not ask, session or always"),
            Why::BrowserNeverAlways => write!(f, "a browser is never `always`; it asks every run"),
            Why::Unreadable(why) => write!(f, "could not be read: {why}"),
        }
    }
}

/// A key or row that was refused and fell back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fallback {
    /// The dotted key (`cua.apps[1]` for a row), `settings.toml` for a file that is not TOML, or
    /// the path of a settings file that could not be read.
    pub key: String,
    /// Why.
    pub why: Why,
}

/// What a read gives: the values, and what was refused or not recognised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// The values in force.
    pub value: CuaSettings,
    /// Keys that fell back to their base value.
    pub fallbacks: Vec<Fallback>,
    /// Dotted paths no key reads (reported, not kept).
    pub unknown: Vec<String>,
}

impl Loaded {
    /// A read of nothing: the base values, nothing to report.
    pub fn of(value: CuaSettings) -> Self {
        Self {
            value,
            fallbacks: Vec::new(),
            unknown: Vec::new(),
        }
    }

    /// One line per fallback and per unknown key, for the daemon's log.
    pub fn lines(&self, daemon: &str) -> Vec<String> {
        let fell = self.fallbacks.iter().map(|f| {
            format!(
                "{daemon}: settings: {}: {}; using the previous value",
                f.key, f.why
            )
        });
        let unknown = self
            .unknown
            .iter()
            .map(|path| format!("{daemon}: settings: {path}: no such key, ignored"));
        fell.chain(unknown).collect()
    }
}

fn find<'a>(table: &'a toml::Table, path: &str) -> Option<&'a toml::Value> {
    let mut parts = path.split('.');
    let mut value = table.get(parts.next()?)?;
    for part in parts {
        value = value.as_table()?.get(part)?;
    }
    Some(value)
}

fn leaves(prefix: &str, table: &toml::Table, out: &mut Vec<String>) {
    for (name, value) in table {
        let path = if prefix.is_empty() {
            name.clone()
        } else {
            format!("{prefix}.{name}")
        };
        match value {
            toml::Value::Table(inner) => leaves(&path, inner, out),
            _ => out.push(path),
        }
    }
}

/// One row of `cua.apps`: the rule, and what was changed or refused about it.
fn row(at: usize, value: &toml::Value) -> (Option<AppRule>, Option<Fallback>) {
    let refuse = |why| {
        (
            None,
            Some(Fallback {
                key: format!("{APPS_KEY}[{at}]"),
                why,
            }),
        )
    };
    let Some(table) = value.as_table() else {
        return refuse(Why::NotAnApp);
    };
    let Some(app) = table
        .get("app")
        .and_then(toml::Value::as_str)
        .and_then(|id| AppName::parse(id).ok())
    else {
        return refuse(Why::NotAnApp);
    };
    let Some(allow) = table
        .get("allow")
        .and_then(toml::Value::as_str)
        .and_then(AppAllow::of_word)
    else {
        return refuse(Why::NotAnAllow);
    };
    if allow == AppAllow::Always && is_browser(&app) {
        let rule = AppRule {
            app,
            allow: AppAllow::Ask,
        };
        let why = Fallback {
            key: format!("{APPS_KEY}[{at}]"),
            why: Why::BrowserNeverAlways,
        };
        return (Some(rule), Some(why));
    }
    (Some(AppRule { app, allow }), None)
}

/// `cua.apps` as the file has it: a list of rows, each read on its own.
fn apps(value: &toml::Value) -> (Option<Apps>, Vec<Fallback>) {
    let Some(rows) = value.as_array() else {
        let why = Fallback {
            key: APPS_KEY.to_owned(),
            why: Why::NotAList,
        };
        return (None, vec![why]);
    };
    let mut rules: Vec<AppRule> = Vec::new();
    let mut fallbacks = Vec::new();
    for (at, value) in rows.iter().enumerate() {
        let (rule, fallback) = row(at, value);
        fallbacks.extend(fallback);
        // A later row for the same app replaces the earlier one.
        if let Some(rule) = rule {
            rules.retain(|r| r.app != rule.app);
            rules.push(rule);
        }
    }
    (Some(Apps::Listed(rules)), fallbacks)
}

/// Reads `text` over `base`: each key the file sets and the table accepts replaces its base
/// value; every other key keeps it. The daemon's `base` lists no app, so until the file names
/// some, none is on (design/22 section 3.29).
pub fn read(text: &str, base: &CuaSettings) -> Loaded {
    let doc = match text.parse::<toml::Table>() {
        Ok(doc) => doc,
        Err(why) => {
            return Loaded {
                value: base.clone(),
                fallbacks: vec![Fallback {
                    key: "settings.toml".to_owned(),
                    why: Why::NotToml(why.message().to_owned()),
                }],
                unknown: Vec::new(),
            };
        }
    };
    let mut value = base.clone();
    let mut fallbacks = Vec::new();
    let keys = table();
    for key in &keys {
        let Some(found) = find(&doc, key.path) else {
            continue;
        };
        let refused = match &key.rule {
            Rule::Number { range, set } => match found.as_integer() {
                None => Some(Why::NotANumber),
                Some(n) if range.contains(&n) => {
                    set(&mut value, n);
                    None
                }
                Some(_) => Some(Why::OutOfRange {
                    min: *range.start(),
                    max: *range.end(),
                }),
            },
            Rule::Word { words, set } => {
                match found
                    .as_str()
                    .and_then(|w| words.iter().position(|x| *x == w))
                {
                    Some(at) => {
                        set(&mut value, at);
                        None
                    }
                    None => Some(Why::NotAWord { words }),
                }
            }
        };
        fallbacks.extend(refused.map(|why| Fallback {
            key: key.path.to_owned(),
            why,
        }));
    }
    if let Some(found) = find(&doc, APPS_KEY) {
        let (listed, refused) = apps(found);
        value.apps = listed.unwrap_or(value.apps);
        fallbacks.extend(refused);
    }
    let known: BTreeSet<&str> = keys
        .iter()
        .map(|k| k.path)
        .chain([APPS_KEY, "version"])
        .collect();
    let mut seen = Vec::new();
    leaves("", &doc, &mut seen);
    let unknown = seen
        .into_iter()
        .filter(|path| !known.contains(path.as_str()))
        .collect();
    Loaded {
        value,
        fallbacks,
        unknown,
    }
}
