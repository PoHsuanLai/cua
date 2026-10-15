//! Where the settings file is: `$XDG_CONFIG_HOME/cua/settings.toml`, then each of
//! `$XDG_CONFIG_DIRS` (design/22 section 2). The first file that reads wins.

use super::read::{Fallback, Loaded, Why, read};
use super::{CuaSettings, SETTINGS_FILE};
use std::path::{Path, PathBuf};

/// The configuration directories, in order of precedence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Locator {
    dirs: Vec<PathBuf>,
}

impl Locator {
    /// The directories named by `XDG_CONFIG_HOME` (else `$HOME/.config`) and `XDG_CONFIG_DIRS`
    /// (else `/etc/xdg`).
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Self {
        let home = env("XDG_CONFIG_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(env("HOME").unwrap_or_default()).join(".config"));
        let rest = env("XDG_CONFIG_DIRS")
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| "/etc/xdg".to_owned());
        Self {
            dirs: std::iter::once(home)
                .chain(rest.split(':').map(PathBuf::from))
                .collect(),
        }
    }

    /// The directory a live watch looks at: the person's own `cua` directory, where the Settings
    /// app writes.
    pub fn watch_dir(&self) -> Option<PathBuf> {
        let person = self.dirs.first()?;
        Path::new(SETTINGS_FILE)
            .parent()
            .map(|name| person.join(name))
    }

    /// The file as it is now over `base`: the first one that reads; no file is `base`, nothing to
    /// report.
    pub fn read(&self, base: &CuaSettings) -> Loaded {
        let mut unreadable = Vec::new();
        let mut text = None;
        for path in self.dirs.iter().map(|d| d.join(SETTINGS_FILE)) {
            match std::fs::read_to_string(&path) {
                Ok(found) => {
                    text = Some(found);
                    break;
                }
                // No file in this directory is the ordinary case.
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                // A file that is there and cannot be read is not the same as no file: it is
                // reported, so a sandbox that hides the settings does not fail silently.
                Err(error) => unreadable.push(Fallback {
                    key: path.display().to_string(),
                    why: Why::Unreadable(error.to_string()),
                }),
            }
        }
        let mut loaded = match text {
            Some(text) => read(&text, base),
            None => Loaded::of(base.clone()),
        };
        unreadable.append(&mut loaded.fallbacks);
        loaded.fallbacks = unreadable;
        loaded
    }
}
