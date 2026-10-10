//! The desktop's shortcut conventions, as a line of plain words for the model.
//!
//! A computer-use model types key chords itself and nothing else tells it which key is the
//! primary one here: on a Mac, and on our desktop, it is the Command key (which reaches the
//! window as Super); elsewhere it is Ctrl. The chords are chordkit's, read from the [`Keymap`]
//! the caller built (the platform's conventions, and the person's own changes when the caller
//! laid them on); nothing here restates one.

use chordkit::{Action, Keymap, StandardAction};

/// The actions a model needs to name a shortcut for, in the order they are listed.
const LISTED: &[StandardAction] = &[
    StandardAction::Copy,
    StandardAction::Cut,
    StandardAction::Paste,
    StandardAction::Undo,
    StandardAction::Redo,
    StandardAction::SelectAll,
    StandardAction::Save,
    StandardAction::Find,
    StandardAction::New,
    StandardAction::Close,
    StandardAction::Quit,
];

/// "Copy" for "copy", "Select all" for "select all".
fn capitalised(words: &str) -> String {
    let mut chars = words.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// One short line telling a model the keyboard shortcuts of the desktop `keymap` is for, in the
/// way the person sees them ("Copy ⌘C" on a Mac, "Copy Ctrl+C" elsewhere), and, where the
/// Command key is the primary one, that it is sent as Super. An action the keymap leaves
/// unbound is left out; `None` when it binds none of them.
pub fn shortcut_hint(keymap: &Keymap) -> Option<String> {
    let platform = keymap.platform();
    let listed: Vec<String> = LISTED
        .iter()
        .filter_map(|action| {
            let chord = keymap
                .chords_of(&Action::Standard(*action))
                .into_iter()
                .next()?;
            Some(format!(
                "{} {}",
                capitalised(&action.label()),
                chord.display(platform)
            ))
        })
        .collect();
    if listed.is_empty() {
        return None;
    }
    let command = match platform.is_mac_style() {
        true => " The \u{2318} key is Super: to press \u{2318}C, send super+c.",
        false => "",
    };
    Some(format!(
        "Shortcuts on this desktop: {}.{command}",
        listed.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chordkit::{Desktop, Platform};

    type Case = (&'static str, Platform, &'static str, &'static str);

    #[test]
    fn each_platform_lists_its_own_chords() {
        let ours = Platform::Linux {
            desktop: Desktop::Ours,
        };
        let other = Platform::Linux {
            desktop: Desktop::Other,
        };
        // (name, platform, what the line must hold, what it must not)
        let cases: &[Case] = &[
            (
                "our desktop: Command, sent as Super",
                ours,
                "Shortcuts on this desktop: Copy \u{2318}C, Cut \u{2318}X, Paste \u{2318}V, Undo \u{2318}Z, Redo \u{21e7}\u{2318}Z, Select all \u{2318}A",
                "Ctrl",
            ),
            (
                "macOS: Command, sent as Super",
                Platform::MacOs,
                "Copy \u{2318}C, Cut \u{2318}X, Paste \u{2318}V",
                "Ctrl",
            ),
            (
                "other Linux: Ctrl",
                other,
                "Shortcuts on this desktop: Copy Ctrl+C, Cut Ctrl+X, Paste Ctrl+V, Undo Ctrl+Z, Redo Ctrl+Shift+Z, Select all Ctrl+A",
                "\u{2318}",
            ),
        ];
        for (name, platform, holds, lacks) in cases {
            let line = shortcut_hint(&Keymap::conventional(*platform)).expect(name);
            assert!(line.contains(holds), "{name}: {line}");
            assert!(!line.contains(lacks), "{name}: {line}");
            assert_eq!(
                line.contains("send super+c"),
                platform.is_mac_style(),
                "{name}: {line}"
            );
        }
    }
}
