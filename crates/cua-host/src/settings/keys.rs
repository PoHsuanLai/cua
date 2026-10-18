//! The key table: every scalar `cua.*` key of design/22 section 3.29 that cuad reads, its rule and
//! where its value goes. One row per schema row (`dist/settings/cua.settings.toml`) except
//! `cua.apps`, which is a list and is read in `apps.rs`/`read.rs`; the tests hold the schema and
//! this table to each other.

use super::CuaSettings;
use a11y_tree::NodeBudget;
use cua_desktop::Ms;
use cua_run::{Autopause, DefaultMode, Enabled};
use porter_core::{Count, MicroUsd};
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

fn count(v: i64) -> Count {
    Count(u32::try_from(v).unwrap_or(u32::MAX))
}

fn ms(v: i64) -> Ms {
    Ms(u32::try_from(v).unwrap_or(u32::MAX))
}

const ENABLED: [Enabled; 2] = [Enabled::Off, Enabled::On];
const MODE: [DefaultMode; 3] = [
    DefaultMode::Auto,
    DefaultMode::InPlace,
    DefaultMode::AgentWorkspace,
];
const AUTOPAUSE: [Autopause; 2] = [Autopause::On, Autopause::Off];

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
        num("cua.budget.steps", 1..=500, |s, v| {
            s.config.budget.steps = count(v)
        }),
        num("cua.budget.active_s", 10..=7_200, |s, v| {
            s.config.budget.active = ms(v.saturating_mul(1000));
        }),
        num("cua.budget.stall_frames", 1..=20, |s, v| {
            s.config.budget.stall_frames = count(v);
        }),
        num("cua.budget.actions_per_min", 1..=600, |s, v| {
            s.config.budget.actions_per_min = count(v);
        }),
        num("cua.budget.model_failures", 0..=10, |s, v| {
            s.config.budget.model_failures = count(v);
        }),
        num("cua.budget.spend_microusd", 0..=1_000_000_000, |s, v| {
            s.config.budget.spend = MicroUsd(u64::try_from(v).unwrap_or(u64::MAX));
        }),
        num("cua.settle.quiet_ms", 50..=2_000, |s, v| {
            s.config.settle.quiet = ms(v)
        }),
        num("cua.settle.timeout_ms", 500..=30_000, |s, v| {
            s.config.settle.timeout = ms(v);
        }),
        num("cua.wait.max_ms", 500..=60_000, |s, v| {
            s.config.wait_max = ms(v)
        }),
        num("cua.wait.total_ms", 1_000..=600_000, |s, v| {
            s.config.wait_total = ms(v)
        }),
        num("cua.tree.max_nodes", 50..=5_000, |s, v| {
            s.config.tree_max_nodes = NodeBudget(u32::try_from(v).unwrap_or(u32::MAX));
        }),
    ]
}
