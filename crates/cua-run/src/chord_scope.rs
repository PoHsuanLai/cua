//! Which key chords act on the desktop rather than on the window they are sent to.
//!
//! A chord is injected into the leased window's seat, but the desktop takes some of them for
//! itself before the window sees them: switching apps, opening the launcher, locking the screen,
//! logging out, switching Spaces. The answer is chordkit's, from the platform's conventions
//! (`Keymap::conventional`) and what the platform reserves (`reserved_reason`); nothing here
//! restates a chord. `Super` is not banned as a whole: on our desktop it is the Command key, and
//! Command+C is an ordinary write.

use chordkit::{
    Action, Chord as KeyChord, Context, Desktop, Key as ChordKey, Keymap, Modifier as KeyModifier,
    Modifiers, NamedKey, Platform, StandardAction, reserved_reason,
};
use cua_action::{Chord, Modifier};
use keyboard_types::Key;
use serde::{Deserialize, Serialize};

/// The keyboard conventions the desktop under the run follows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyWorld {
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
    /// KDE Plasma.
    Kde,
    /// GNOME.
    Gnome,
    /// This project's desktop: the Command key arrives as Super.
    #[default]
    Ours,
    /// Any other Linux desktop.
    OtherLinux,
}

impl KeyWorld {
    fn platform(self) -> Platform {
        let linux = |desktop| Platform::Linux { desktop };
        match self {
            KeyWorld::MacOs => Platform::MacOs,
            KeyWorld::Windows => Platform::Windows,
            KeyWorld::Kde => linux(Desktop::Kde),
            KeyWorld::Gnome => linux(Desktop::Gnome),
            KeyWorld::Ours => linux(Desktop::Ours),
            KeyWorld::OtherLinux => linux(Desktop::Other),
        }
    }
}

/// Whether the standard action is done by the desktop as a whole (apps, windows, Spaces, the
/// session, the screen) and not by the app that has the focus.
fn desktop_wide(action: StandardAction) -> bool {
    matches!(
        action,
        StandardAction::Launcher
            | StandardAction::NextInputSource
            | StandardAction::NextApp
            | StandardAction::PreviousApp
            | StandardAction::NextWindow
            | StandardAction::MinimizeAll
            | StandardAction::HideOthers
            | StandardAction::ToggleDock
            | StandardAction::ScreenshotScreen
            | StandardAction::ScreenshotSelection
            | StandardAction::ScreenshotTools
            | StandardAction::ForceQuit
            | StandardAction::LockScreen
            | StandardAction::LogOut
            | StandardAction::MissionControl
            | StandardAction::SpaceLeft
            | StandardAction::SpaceRight
            | StandardAction::SwitchToSpace(_)
    )
}

fn modifiers_of(chord: &Chord) -> Modifiers {
    chord
        .mods
        .iter()
        .map(|m| match m {
            Modifier::Ctrl => KeyModifier::Ctrl,
            Modifier::Alt => KeyModifier::Alt,
            Modifier::Shift => KeyModifier::Shift,
            Modifier::Super => KeyModifier::Super,
        })
        .collect()
}

/// The key as chordkit writes it; `None` for a key no desktop shortcut here uses.
fn key_of(key: &Key) -> Option<ChordKey> {
    let named = |named| Some(ChordKey::Named(named));
    match key {
        Key::Character(text) => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(' '), None) => named(NamedKey::Space),
                (Some(c), None) => Some(ChordKey::character(c)),
                _ => None,
            }
        }
        Key::Tab => named(NamedKey::Tab),
        Key::Enter => named(NamedKey::Enter),
        Key::Escape => named(NamedKey::Escape),
        Key::ArrowLeft => named(NamedKey::Left),
        Key::ArrowRight => named(NamedKey::Right),
        Key::ArrowUp => named(NamedKey::Up),
        Key::ArrowDown => named(NamedKey::Down),
        Key::Delete => named(NamedKey::Delete),
        Key::Super | Key::Meta => Some(ChordKey::Tap(KeyModifier::Super)),
        other => function_key(other),
    }
}

fn function_key(key: &Key) -> Option<ChordKey> {
    let n = match key {
        Key::F1 => 1,
        Key::F2 => 2,
        Key::F3 => 3,
        Key::F4 => 4,
        Key::F5 => 5,
        Key::F6 => 6,
        Key::F7 => 7,
        Key::F8 => 8,
        Key::F9 => 9,
        Key::F10 => 10,
        Key::F11 => 11,
        Key::F12 => 12,
        _ => return None,
    };
    ChordKey::function(n)
}

/// Whether `chord` is one the desktop acts on itself in `world`: bound to a desktop-wide action
/// in the platform's conventions, or reserved by the platform. Ordinary shortcuts (copy, paste,
/// undo, save, find) are not.
pub fn acts_on_desktop(chord: &Chord, world: KeyWorld) -> bool {
    let Some(key) = key_of(&chord.key) else {
        return false;
    };
    let pressed = KeyChord::new(modifiers_of(chord), key);
    let platform = world.platform();
    if reserved_reason(platform, pressed).is_some() {
        return true;
    }
    Keymap::conventional(platform)
        .bindings(Context::Normal)
        .iter()
        .any(|(action, bound)| {
            *bound == pressed && matches!(action, Action::Standard(a) if desktop_wide(*a))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn chord(mods: &[Modifier], key: Key) -> Chord {
        Chord::new(mods.iter().copied().collect::<BTreeSet<_>>(), key).expect("a chord")
    }

    fn letter(c: &str) -> Key {
        Key::Character(c.to_owned())
    }

    #[test]
    fn the_desktops_own_chords_are_told_from_the_window_shortcuts() {
        use Modifier::{Alt, Ctrl, Shift, Super};
        // (chord, world, acts on the desktop)
        let table = [
            (chord(&[Super], letter("c")), KeyWorld::Ours, false),
            (chord(&[Super], letter("v")), KeyWorld::Ours, false),
            (chord(&[Super], letter("z")), KeyWorld::Ours, false),
            (chord(&[Ctrl], letter("c")), KeyWorld::Kde, false),
            (chord(&[], Key::Enter), KeyWorld::Ours, false),
            (chord(&[Super], Key::Tab), KeyWorld::Ours, true),
            (chord(&[Shift, Super], Key::Tab), KeyWorld::Ours, true),
            (chord(&[Super], letter(" ")), KeyWorld::Ours, true),
            (chord(&[Ctrl], letter("1")), KeyWorld::Ours, true),
            (chord(&[Alt], Key::Tab), KeyWorld::Kde, true),
            (chord(&[Ctrl, Alt], Key::Delete), KeyWorld::Kde, true),
            (chord(&[], Key::Super), KeyWorld::Ours, true),
        ];
        for (chord, world, want) in table {
            assert_eq!(acts_on_desktop(&chord, world), want, "{chord:?} {world:?}");
        }
    }
}
