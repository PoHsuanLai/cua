//! The proposed values the specs name, as one typed configuration with its defaults, and the
//! table of settings rows that carry them. cua never reads settings itself: sill's Intelligence
//! page owns the keys (design/22 rows), and cuad receives a [`CuaConfig`].
//!
//! `cua.budget.denials` is not here: the router's breaker (`agent.breaker.*`) counts denials.

use a11y_tree::NodeBudget;
use cua_bus::RunBudget;
use cua_desktop::Ms;
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

/// `cua.accessibility`: whether a run reads the controls (buttons, fields) of the window it is
/// using, as the app describes them. Only that window's; with it off a run goes from pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Accessibility {
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
    /// No change for this long means settled.
    pub quiet: Ms,
    /// Give up after this long.
    pub timeout: Ms,
}

/// How long to wait for the screen to settle after an action: quiet for 250 ms, at most 3 s.
pub const SETTLE: SettleLimits = SettleLimits {
    quiet: Ms(250),
    timeout: Ms(3000),
};

/// The longest one `wait` may last: 60 s, the most the action can ask.
pub const WAIT_MAX: Ms = Ms(60_000);

/// How long a run may keep waiting, `wait` after `wait`, before an unchanged screen counts as
/// stalled again: ten minutes.
pub const WAIT_TOTAL: Ms = Ms(600_000);

/// How many tree nodes a snapshot keeps.
pub const TREE_MAX_NODES: NodeBudget = NodeBudget(400);

/// Every proposed value of computer use in one place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuaConfig {
    /// `cua.enabled`.
    pub enabled: Enabled,
    /// `cua.default_mode`.
    pub default_mode: DefaultMode,
    /// `cua.budget.*`.
    pub budget: RunBudget,
    /// `cua.autopause`.
    pub autopause: Autopause,
    /// `cua.accessibility` (on).
    pub accessibility: Accessibility,
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
            autopause: Autopause::On,
            accessibility: Accessibility::On,
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
    n("cua.budget.spend_microusd", 0),
    w("cua.autopause", "on"),
    w("cua.accessibility", "on"),
    w("cua.screens.keep", "off"),
    n("cua.screens.keep_days", 7),
];

impl CuaConfig {
    /// The value of `key` in this configuration; `None` for a key cuad does not read.
    pub fn value(&self, key: &str) -> Option<SettingValue> {
        let number = |v: u64| i64::try_from(v).ok().map(SettingValue::Number);
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
            "cua.budget.spend_microusd" => number(self.budget.spend.0),
            "cua.autopause" => Some(SettingValue::Word(match self.autopause {
                Autopause::Off => "off",
                Autopause::On => "on",
            })),
            "cua.accessibility" => Some(SettingValue::Word(match self.accessibility {
                Accessibility::Off => "off",
                Accessibility::On => "on",
            })),
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
