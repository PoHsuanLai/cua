//! The key table: every scalar `cua.*` key of design/22 section 3.29 that cuad reads, its rule and
//! where its value goes. One row per schema row (`dist/settings/cua.settings.toml`) except
//! `cua.apps`, which is a list and is read in `apps.rs`/`read.rs`; the tests hold the schema and
//! this table to each other.

use super::CuaSettings;
use model_provider::Money;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use std::ops::RangeInclusive;

/// What a key's value may be and how it lands in the settings.
#[derive(Clone)]
pub(crate) enum Rule {
    /// A whole number inside `range`, in the key's unit.
    Number {
        range: RangeInclusive<i64>,
        set: fn(&mut CuaSettings, i64),
    },
    /// One of `words`: `set` reads the word through serde and says whether the field took it.
    Word {
        words: &'static [&'static str],
        set: fn(&mut CuaSettings, &str) -> bool,
    },
}

/// One key of the table.
#[derive(Clone)]
pub(crate) struct Key {
    pub path: &'static str,
    pub rule: Rule,
}

/// The list key, read apart from the table.
pub(crate) const APPS_KEY: &str = "cua.apps";

fn num(path: &'static str, range: RangeInclusive<i64>, set: fn(&mut CuaSettings, i64)) -> Key {
    Key {
        path,
        rule: Rule::Number { range, set },
    }
}

fn word(
    path: &'static str,
    words: &'static [&'static str],
    set: fn(&mut CuaSettings, &str) -> bool,
) -> Key {
    Key {
        path,
        rule: Rule::Word { words, set },
    }
}

/// Reads `word` as the serde form of `T`; `None` when it is not one of `T`'s words.
fn read_word<T: DeserializeOwned>(word: &str) -> Option<T> {
    toml::Value::String(word.to_owned()).try_into().ok()
}

/// Every scalar key, in the order the schema lists them.
pub(crate) fn table() -> Vec<Key> {
    vec![
        word("cua.enabled", &["off", "on"], |s, w| {
            read_word(w).map(|v| s.config.enabled = v).is_some()
        }),
        word(
            "cua.default_mode",
            &["auto", "in_place", "agent_workspace"],
            |s, w| read_word(w).map(|v| s.config.default_mode = v).is_some(),
        ),
        word("cua.autopause", &["on", "off"], |s, w| {
            read_word(w).map(|v| s.config.autopause = v).is_some()
        }),
        word("cua.accessibility", &["on", "off"], |s, w| {
            read_word(w).map(|v| s.config.accessibility = v).is_some()
        }),
        num("cua.budget.spend_microusd", 0..=1_000_000_000, |s, v| {
            // Millionths of a US dollar in the file; 0 is a cap of nothing (local models only).
            s.config.budget.spend = Money::usd(Decimal::new(v, 6));
        }),
    ]
}
