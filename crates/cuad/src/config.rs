//! cuad's configuration: the settings sill's Intelligence page owns, plus which backend and who
//! may call. Parsed once, at the edge.

use crate::callers::CallerNames;
use cua_desktop::BackendKind;
use cua_run::CuaConfig;
use porter_core::Tier;
use serde::{Deserialize, Serialize};

/// Which backend cuad uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BackendChoice {
    /// Detect: the fork when the compositor offers `quire_agent_manager_v1`, otherwise stock
    /// cosmic-comp, otherwise none.
    Detect,
    /// This one, whatever the compositor offers.
    Only(BackendKind),
}

/// Everything cuad reads at start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuadConfig {
    /// The settings (`cua.*`).
    pub settings: CuaConfig,
    /// Which backend.
    pub backend: BackendChoice,
    /// The tier asked of inferd for the computer-use need (`Need::ComputerUse`, class `Screen`).
    pub tier: Tier,
    /// Which bus names are the router, the planner and the shell.
    pub callers: CallerNames,
}

impl Default for CuadConfig {
    fn default() -> Self {
        Self {
            settings: CuaConfig::default(),
            backend: BackendChoice::Detect,
            tier: Tier::Balanced,
            callers: CallerNames::default(),
        }
    }
}

/// Why a configuration did not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cuad.toml: {0}")]
pub struct ConfigError(pub String);

impl CuadConfig {
    /// Reads a configuration from TOML text; a missing file is the caller's decision, a
    /// malformed one is an error here.
    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        toml::from_str(text).map_err(|e| ConfigError(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_configuration_is_off_and_detects_its_backend() {
        let config = CuadConfig::default();
        assert_eq!(config.settings.enabled, cua_run::Enabled::Off);
        assert_eq!(config.backend, BackendChoice::Detect);
    }

    #[test]
    fn a_configuration_round_trips_through_toml() {
        let config = CuadConfig::default();
        let text = toml::to_string(&config).expect("toml");
        assert_eq!(CuadConfig::from_toml(&text), Ok(config));
        assert!(CuadConfig::from_toml("settings = 3").is_err());
    }
}
