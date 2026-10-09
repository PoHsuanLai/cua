//! The proposed values the specs name, as one typed configuration with its defaults, and the
//! table of settings rows that carry them. cua never reads settings itself: sill's Intelligence
//! page owns the keys (design/22 rows), and cuad receives a [`CuaConfig`].
//!
//! `cua.budget.denials` is not here: the router's breaker (`agent.breaker.*`) counts denials.

use a11y_tree::NodeBudget;
use cua_bus::RunBudget;
use cua_desktop::Ms;
use porter_core::Count;
use serde::{Deserialize, Serialize};

/// `cua.enabled`: computer use is off until the person turns it on, and then still needs the
/// per-app consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Enabled {
    /// Off: `Cua1.Start` answers `Disabled` and the `org.quire.Cua` action is hidden.
    Off,
    /// On.
    On,
}

/// `cua.default_mode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultMode {
    /// The agent workspace where the backend fully supports it, otherwise in place.
    Auto,
    /// In place.
    InPlace,
    /// The agent workspace.
    AgentWorkspace,
}

/// `cua.autopause`: whether physical input pauses a run on a shared seat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Autopause {
    /// It does not.
    Off,
    /// It does.
    On,
}

/// `cua.screens.keep`: whether thumbnails of steps are stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeepScreens {
    /// No pixels are stored.
    Off,
    /// Thumbnails are kept in the Space's encrypted store.
    On,
}

/// A number of days.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Days(pub u32);

/// How long to wait for the screen to settle after an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SettleLimits {
    /// `cua.settle.quiet_ms`: no change for this long means settled (250).
    pub quiet: Ms,
    /// `cua.settle.timeout_ms`: give up after this long (3000).
    pub timeout: Ms,
}

/// Every proposed value of computer use in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuaConfig {
    /// `cua.enabled`.
    pub enabled: Enabled,
    /// `cua.default_mode`.
    pub default_mode: DefaultMode,
    /// `cua.budget.*`.
    pub budget: RunBudget,
    /// `cua.settle.*`.
    pub settle: SettleLimits,
    /// `cua.wait.max_ms`: the longest a model may ask to wait (5000).
    pub wait_max: Ms,
    /// `cua.autopause`.
    pub autopause: Autopause,
    /// `cua.tree.max_nodes` (400).
    pub tree_max_nodes: NodeBudget,
    /// `cua.screens.keep`.
    pub keep_screens: KeepScreens,
    /// `cua.screens.keep_days` (7).
    pub keep_days: Days,
}

impl Default for CuaConfig {
    fn default() -> Self {
        CuaConfig {
            enabled: Enabled::Off,
            default_mode: DefaultMode::Auto,
            budget: RunBudget::PROPOSED,
            settle: SettleLimits {
                quiet: Ms(250),
                timeout: Ms(3000),
            },
            wait_max: Ms(5000),
            autopause: Autopause::On,
            tree_max_nodes: NodeBudget(400),
            keep_screens: KeepScreens::Off,
            keep_days: Days(7),
        }
    }
}

/// A settings value as a row carries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingValue {
    /// A number in the row's unit.
    Number(i64),
    /// One word of a closed set.
    Word(&'static str),
}

/// One settings row cuad reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingRow {
    /// The key.
    pub key: &'static str,
    /// The proposed default.
    pub default: SettingValue,
}

const fn n(key: &'static str, v: i64) -> SettingRow {
    SettingRow {
        key,
        default: SettingValue::Number(v),
    }
}

const fn w(key: &'static str, v: &'static str) -> SettingRow {
    SettingRow {
        key,
        default: SettingValue::Word(v),
    }
}

/// Every row, in the order the Intelligence page lists them.
pub const SETTING_ROWS: &[SettingRow] = &[
    w("cua.enabled", "off"),
    w("cua.default_mode", "auto"),
    n("cua.budget.steps", 40),
    n("cua.budget.active_s", 600),
    n("cua.budget.stall_frames", 3),
    n("cua.budget.actions_per_min", 60),
    n("cua.budget.model_failures", 2),
    n("cua.budget.spend_microusd", 0),
    n("cua.settle.quiet_ms", 250),
    n("cua.settle.timeout_ms", 3000),
    n("cua.wait.max_ms", 5000),
    w("cua.autopause", "on"),
    n("cua.tree.max_nodes", 400),
    w("cua.screens.keep", "off"),
    n("cua.screens.keep_days", 7),
];

impl CuaConfig {
    /// The value of `key` in this configuration; `None` for a key cuad does not read.
    pub fn value(&self, key: &str) -> Option<SettingValue> {
        let number = |v: u64| i64::try_from(v).ok().map(SettingValue::Number);
        let count = |c: Count| Some(SettingValue::Number(i64::from(c.0)));
        match key {
            "cua.enabled" => Some(SettingValue::Word(match self.enabled {
                Enabled::Off => "off",
                Enabled::On => "on",
            })),
            "cua.default_mode" => Some(SettingValue::Word(match self.default_mode {
                DefaultMode::Auto => "auto",
                DefaultMode::InPlace => "in_place",
                DefaultMode::AgentWorkspace => "agent_workspace",
            })),
            "cua.budget.steps" => count(self.budget.steps),
            "cua.budget.active_s" => {
                Some(SettingValue::Number(i64::from(self.budget.active.0 / 1000)))
            }
            "cua.budget.stall_frames" => count(self.budget.stall_frames),
            "cua.budget.actions_per_min" => count(self.budget.actions_per_min),
            "cua.budget.model_failures" => count(self.budget.model_failures),
            "cua.budget.spend_microusd" => number(self.budget.spend.0),
            "cua.settle.quiet_ms" => Some(SettingValue::Number(i64::from(self.settle.quiet.0))),
            "cua.settle.timeout_ms" => Some(SettingValue::Number(i64::from(self.settle.timeout.0))),
            "cua.wait.max_ms" => Some(SettingValue::Number(i64::from(self.wait_max.0))),
            "cua.autopause" => Some(SettingValue::Word(match self.autopause {
                Autopause::Off => "off",
                Autopause::On => "on",
            })),
            "cua.tree.max_nodes" => Some(SettingValue::Number(i64::from(self.tree_max_nodes.0))),
            "cua.screens.keep" => Some(SettingValue::Word(match self.keep_screens {
                KeepScreens::Off => "off",
                KeepScreens::On => "on",
            })),
            "cua.screens.keep_days" => Some(SettingValue::Number(i64::from(self.keep_days.0))),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_settings_row_carries_the_default_of_the_configuration_and_a_unique_namespaced_key() {
        let config = CuaConfig::default();
        let mut seen = std::collections::BTreeSet::new();
        for row in SETTING_ROWS {
            assert_eq!(config.value(row.key), Some(row.default), "row {}", row.key);
            assert!(seen.insert(row.key), "duplicate {}", row.key);
            assert!(row.key.starts_with("cua."), "{}", row.key);
        }
    }

    #[test]
    fn computer_use_is_off_by_default_and_the_breaker_not_the_run_counts_denials() {
        let config = CuaConfig::default();
        assert_eq!(config.enabled, Enabled::Off);
        assert_eq!(config.keep_screens, KeepScreens::Off);
        assert_eq!(config.value("cua.budget.denials"), None);
    }

    #[test]
    fn the_configuration_round_trips() {
        let config = CuaConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<CuaConfig>(&json).unwrap(), config);
        assert!(json.starts_with(r#"{"enabled":"off","default_mode":"auto","budget":{"steps":40"#));
    }
}
