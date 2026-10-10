//! The keymap cuad tells its model about: the platform's conventions and, where the `keycap`
//! feature is on, this desktop's own bindings and the person's overrides, read once at start.

use chordkit::{Keymap, Platform};

/// The keymap of `platform`, with every problem found reading it in plain words (nothing to
/// report without the `keycap` feature). Environment variables are read through `lookup`; the
/// daemon's `main` passes the process's.
///
/// With the feature, the person's keymap comes from keycap's daemon, or its files when none
/// answers; without it, only the platform's conventions apply.
#[cfg(feature = "keycap")]
pub fn desktop_keymap(
    platform: Platform,
    lookup: impl Fn(&str) -> Option<String>,
) -> (Keymap, Vec<String>) {
    keycap_client::KeycapSource::from_lookup(lookup).load_reporting(platform)
}

/// The keymap of `platform`: its conventions only (the `keycap` feature is off).
#[cfg(not(feature = "keycap"))]
pub fn desktop_keymap(
    platform: Platform,
    lookup: impl Fn(&str) -> Option<String>,
) -> (Keymap, Vec<String>) {
    let _ = lookup;
    (Keymap::conventional(platform), Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordkit::{Action, Desktop, StandardAction};

    const OURS: Platform = Platform::Linux {
        desktop: Desktop::Ours,
    };

    fn copy_of(keymap: &Keymap) -> Vec<String> {
        keymap
            .chords_of(&Action::Standard(StandardAction::Copy))
            .iter()
            .map(|chord| chord.display(OURS))
            .collect()
    }

    #[test]
    fn nothing_found_is_the_conventions() {
        let nowhere = |name: &str| match name {
            "KEYCAP_SOCKET" => Some("/nonexistent/keycap.sock".to_owned()),
            "KEYCAP_CONFIG_DIR" => Some("/nonexistent/keycap".to_owned()),
            _ => None,
        };
        let (keymap, _) = desktop_keymap(OURS, nowhere);
        assert!(copy_of(&keymap).contains(&"\u{2318}C".to_owned()));
    }

    #[cfg(feature = "keycap")]
    #[test]
    fn the_persons_own_keymap_file_is_laid_over_the_conventions() {
        let home = tempfile::tempdir().expect("a scratch directory");
        let dir = home.path().join("keycap");
        std::fs::create_dir_all(&dir).expect("the keycap directory");
        std::fs::write(dir.join("keymap"), "copy = Primary+Alt+C\n").expect("the keymap file");
        let root = home.path().to_string_lossy().into_owned();
        let lookup = |name: &str| match name {
            "XDG_CONFIG_HOME" => Some(root.clone()),
            "KEYCAP_SOCKET" => Some(format!("{root}/no-socket")),
            "KEYCAP_CONFIG_DIR" => Some(format!("{root}/no-machine-dir")),
            _ => None,
        };
        let (keymap, _) = desktop_keymap(OURS, lookup);
        assert_eq!(copy_of(&keymap), ["\u{2325}\u{2318}C".to_owned()]);
    }
}
