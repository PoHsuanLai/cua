//! What the log keeps of an action: typed text is never kept, only its length and a digest.

use cua_action::{CuaAction, TypedText};
use cua_bus::TargetedAction;

/// The action as the log keeps it: typed text becomes a note of its length and the blake3 digest
/// of the text (`[12 chars, blake3 ...]`), so a typed secret is not in the event log and two
/// records of the same text can still be told equal.
pub(crate) fn kept_action(action: &TargetedAction) -> TargetedAction {
    let CuaAction::Type { text } = &action.action else {
        return action.clone();
    };
    let note = format!(
        "[{} chars, blake3 {}]",
        text.as_str().chars().count(),
        blake3::hash(text.as_str().as_bytes()).to_hex()
    );
    let action_kept = match TypedText::new(note) {
        Ok(text) => CuaAction::Type { text },
        // Not reachable (the note is short and plain); the text is never the fallback.
        Err(_) => CuaAction::Observe,
    };
    TargetedAction {
        window: action.window,
        action: action_kept,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_bus::WindowRef;

    #[test]
    fn typed_text_is_kept_as_a_digest_and_other_actions_as_they_are() {
        let typed = TargetedAction {
            window: WindowRef(1),
            action: CuaAction::Type {
                text: TypedText::new("hunter2").expect("text"),
            },
        };
        let kept = format!("{:?}", kept_action(&typed));
        let CuaAction::Type { text } = kept_action(&typed).action else {
            panic!("still a Type action");
        };
        assert!(text.as_str().starts_with("[7 chars, blake3 "), "{kept}");
        assert!(!text.as_str().contains("hunter2"));
        let observe = TargetedAction {
            window: WindowRef(1),
            action: CuaAction::Observe,
        };
        assert_eq!(kept_action(&observe), observe);
    }
}
