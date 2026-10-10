//! The keymap cuad tells its model about: the platform's conventions and, where the `keycap`
//! feature is on, this desktop's own bindings and the person's overrides, read once at start.
//!
//! cuad uses the standard shortcuts only (owner decision, 2026-10-10). Its sandbox
//! (`dist/cuad.service`) keeps the person's home and the keycap socket closed, so their own
//! remappings are not read and the shortcut hints come from chordkit's conventional keymap (with
//! the desktop's own bindings). A source that cannot be opened is therefore not a problem: it is
//! skipped without a word, and only what was read and found wrong (a bad line in a keymap file
//! that did open) is reported.

use chordkit::{Keymap, Platform};

/// What keycap-client says of a keymap file it could not open (`couldn't be used`) and the
/// operating system's own reason (`os error`). Under the sandbox every closed source says it.
#[cfg(feature = "keycap")]
fn is_closed_source(problem: &str) -> bool {
    problem.contains("couldn't be used") && problem.contains("(os error")
}

/// The keymap of `platform`, with every problem found reading it in plain words (nothing to
/// report without the `keycap` feature). Environment variables are read through `lookup`; the
/// daemon's `main` passes the process's.
///
/// With the feature, the person's keymap comes from keycap's daemon, or its files when none
/// answers; a daemon or file that is closed to cuad (its sandbox) leaves the conventional keymap
/// and no problem. Without the feature, only the platform's conventions apply.
#[cfg(feature = "keycap")]
pub fn desktop_keymap(
    platform: Platform,
    lookup: impl Fn(&str) -> Option<String>,
) -> (Keymap, Vec<String>) {
    let (keymap, problems) =
        keycap_client::KeycapSource::from_lookup(lookup).load_reporting(platform);
    let read = problems
        .into_iter()
        .filter(|problem| !is_closed_source(problem))
        .collect();
    (keymap, read)
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

    #[cfg(feature = "keycap")]
    #[test]
    fn sources_closed_to_cuad_leave_the_conventions_and_say_nothing() {
        // A path below a regular file cannot be opened (not "missing"): what the sandbox does to
        // the person's home.
        let home = tempfile::tempdir().expect("a scratch directory");
        let blocker = home.path().join("closed");
        std::fs::write(&blocker, "x").expect("a plain file");
        let root = blocker.to_string_lossy().into_owned();
        let lookup = |name: &str| match name {
            "XDG_CONFIG_HOME" => Some(root.clone()),
            "KEYCAP_SOCKET" => Some(format!("{root}/no-socket")),
            "KEYCAP_CONFIG_DIR" => Some(format!("{root}/no-machine-dir")),
            _ => None,
        };
        let (keymap, problems) = desktop_keymap(OURS, lookup);
        assert_eq!(problems, Vec::<String>::new());
        assert!(copy_of(&keymap).contains(&"\u{2318}C".to_owned()));
    }

    #[cfg(feature = "keycap")]
    #[test]
    fn a_closed_source_reads_as_closed_and_a_bad_line_does_not() {
        assert!(is_closed_source(
            "Your keymap: The keymap file couldn't be used (Permission denied (os error 13))."
        ));
        assert!(!is_closed_source("Your keymap: line 2: not a binding"));
        assert!(!is_closed_source(
            "Your keymap: The keymap file couldn't be used (it is larger than 64 KiB)."
        ));
    }
}
