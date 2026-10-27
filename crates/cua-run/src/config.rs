//! The proposed values the specs name, as one typed configuration with its defaults. This crate
//! never reads settings: `cua-host` holds the key table and the schema, and cuad receives a
//! [`CuaConfig`].
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computer_use_is_off_by_default_and_the_breaker_not_the_run_counts_denials() {
        let config = CuaConfig::default();
        assert_eq!(config.enabled, Enabled::Off);
        assert_eq!(config.keep_screens, KeepScreens::Off);
    }

    #[test]
    fn the_configuration_round_trips() {
        let config = CuaConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        assert_eq!(serde_json::from_str::<CuaConfig>(&json).unwrap(), config);
        assert!(json.starts_with(r#"{"enabled":"off","default_mode":"auto","budget":{"steps":40"#));
    }
}
