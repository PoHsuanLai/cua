//! The person's computer-use settings (design/22 section 3.29): the `cua.*` keys the Settings app
//! writes to `$XDG_CONFIG_HOME/cua/settings.toml`, read leniently (a bad value falls back, per key,
//! and is logged) and found through the configuration directories. The pure part (`read`) is text
//! in, typed values out; the file is the edge, and the live watch on it is the daemon's, which
//! feeds a [`SettingsFeed`] to the supervisor.

mod apps;
mod keys;
mod locate;
mod read;
#[cfg(test)]
mod tests;

use cua_run::CuaConfig;

pub use apps::{AppAllow, AppRule, Apps, is_browser};
pub use locate::Locator;
pub use read::{Fallback, Loaded, Why, read};

/// The schema cua ships for its settings (design/22 section 9.2).
pub const SCHEMA: &str = include_str!("../../../../dist/settings/cua.settings.toml");

/// The file the settings live in, under the configuration directory.
pub const SETTINGS_FILE: &str = "cua/settings.toml";

/// Where the settings changes come from: a directory watch in the daemon, a test's channel, an
/// app's own settings store.
pub trait SettingsFeed: Send {
    /// Waits for the next change, the whole file read again; `None` once the feed has ended.
    fn changed(&mut self) -> impl Future<Output = Option<Loaded>> + Send;
}

/// Every value of the file, typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CuaSettings {
    /// The proposed values, with the person's choices in them.
    pub config: CuaConfig,
    /// `cua.apps`: the apps computer use may drive.
    pub apps: Apps,
}

impl CuaSettings {
    /// The settings of a machine with no file: `config`'s values and no restriction on apps (what
    /// a supervisor built in code runs under; the daemon reads the file and lists the apps).
    pub fn over(config: CuaConfig) -> Self {
        Self {
            config,
            apps: Apps::Unrestricted,
        }
    }
}
