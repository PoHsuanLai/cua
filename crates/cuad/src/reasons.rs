//! The words a refused `cua.run.start` gives the person (and the planner, as the app's own
//! reason): one plain sentence, what is wrong and where to put it right when there is a place.
//! No ids, no developer words.

use cua_bus::ControlRefusal;
use cua_run::Enabled;

/// Why a start was refused, in words. `enabled` is the setting now: `Disabled` means the whole
/// feature is off when it is, and that the app is not on the person's list when it is on.
pub(crate) fn words_of(why: ControlRefusal, enabled: Enabled) -> &'static str {
    match (why, enabled) {
        (ControlRefusal::Disabled, Enabled::Off) => {
            "On-screen control is off. You can turn it on in Settings."
        }
        (ControlRefusal::Disabled, Enabled::On) => {
            "That app isn't allowed for on-screen control. You can allow it in Settings."
        }
        (ControlRefusal::NotPermitted, _) => "On-screen control wasn't allowed to start.",
        (ControlRefusal::WindowUnknown, _) => "That window isn't open any more.",
        (ControlRefusal::LeaseHeld, _) => "Another on-screen task is already using that window.",
        (ControlRefusal::ModeUnsupported, _) => {
            "This computer can't do on-screen control that way."
        }
        (ControlRefusal::BadTask, _) => "On-screen control needs a window or an app to work in.",
        (ControlRefusal::WrongState | ControlRefusal::UnknownQuestion, _) => {
            "On-screen control isn't ready for that."
        }
        // A refusal this build does not know is still a refusal.
        _ => "On-screen control couldn't start.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_refusal_has_a_plain_sentence_and_disabled_says_which_setting() {
        for enabled in [Enabled::Off, Enabled::On] {
            for why in ControlRefusal::ALL {
                let words = words_of(why, enabled);
                assert!(words.ends_with('.'), "{why:?}: {words}");
                for word in ["Disabled", "Lease", "BadTask", "cua", "D-Bus", "org."] {
                    assert!(!words.contains(word), "{why:?}: {words}");
                }
            }
        }
        assert!(
            words_of(ControlRefusal::Disabled, Enabled::Off).contains("turn it on in Settings")
        );
        assert!(words_of(ControlRefusal::Disabled, Enabled::On).contains("allow it in Settings"));
    }
}
