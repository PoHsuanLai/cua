//! The key table: every scalar `cua.*` key of design/22 section 3.29 that cuad reads, its rule and
//! where its value goes. One row per schema row (`dist/settings/cua.settings.toml`) except
//! `cua.apps`, which is a list and is read in `apps.rs`/`read.rs`; the tests hold the schema and
//! this table to each other.

use super::CuaSettings;
use cua_run::{Accessibility, Autopause, DefaultMode, Enabled};
use porter_core::MicroUsd;
use std::ops::RangeInclusive;

/// What a key's value may be and how it lands in the settings.
#[derive(Clone)]
pub(crate) enum Rule {
    /// A whole number inside `range`, in the key's unit.
    Number {
        range: RangeInclusive<i64>,
        set: fn(&mut CuaSettings, i64),
    },
    /// One of `words`, by position.
    Word {
        words: &'static [&'static str],
        set: fn(&mut CuaSettings, usize),
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
    set: fn(&mut CuaSettings, usize),
) -> Key {
    Key {
        path,
        rule: Rule::Word { words, set },
    }
}

const ENABLED: [Enabled; 2] = [Enabled::Off, Enabled::On];
const MODE: [DefaultMode; 3] = [
    DefaultMode::Auto,
    DefaultMode::InPlace,
    DefaultMode::AgentWorkspace,
];
const AUTOPAUSE: [Autopause; 2] = [Autopause::On, Autopause::Off];
const ACCESSIBILITY: [Accessibility; 2] = [Accessibility::On, Accessibility::Off];

/// Every scalar key, in the order the schema lists them.
pub(crate) fn table() -> Vec<Key> {
    vec![
        word("cua.enabled", &["off", "on"], |s, i| {
            s.config.enabled = ENABLED[i];
        }),
        word(
            "cua.default_mode",
            &["auto", "in_place", "agent_workspace"],
            |s, i| s.config.default_mode = MODE[i],
        ),
        word("cua.autopause", &["on", "off"], |s, i| {
            s.config.autopause = AUTOPAUSE[i];
        }),
        word("cua.accessibility", &["on", "off"], |s, i| {
            s.config.accessibility = ACCESSIBILITY[i];
        }),
        num("cua.budget.spend_microusd", 0..=1_000_000_000, |s, v| {
            s.config.budget.spend = MicroUsd(u64::try_from(v).unwrap_or(u64::MAX));
        }),
    ]
}
