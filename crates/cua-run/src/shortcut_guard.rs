//! A key chord for something the window already shows as a control.
//!
//! A model that presses Ctrl+N for "new" gets whatever the desktop does with it, and nothing on
//! the screen says whether the chord worked. When the tree shows a visible, enabled control with
//! the same meaning ("New Tab"), the run says so and points at the control instead. With no
//! tree the guard says nothing: a chord is never blocked blindly.

use crate::chord_scope::standard_actions_of;
use a11y_tree::{A11yNode, NodeState, Role};
use chordkit::StandardAction;
use cua_action::{CuaAction, NodeId, WindowSpace};

/// The standard actions that make or open something in the window, which a control usually does
/// too. Editing chords (copy, paste, undo) are not here: they act on the focus, not on a control.
const GUARDED: &[StandardAction] = &[StandardAction::New, StandardAction::Open];

/// The roles of a control that can be clicked.
const CLICKABLE: &[Role] = &[
    Role::Button,
    Role::MenuItem,
    Role::Tab,
    Role::Link,
    Role::ToggleButton,
];

/// The control that does what a chord would.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    /// Its id in the tree the model reads.
    pub id: NodeId,
    /// Its name, screen text.
    pub name: String,
}

impl Control {
    /// The words for the model: which control to click instead.
    pub fn words(&self) -> String {
        format!(
            "that shortcut is not needed here: click {:?} [{}] instead",
            self.name, self.id.0
        )
    }
}

fn is_shown(node: &A11yNode) -> bool {
    [NodeState::Visible, NodeState::Showing, NodeState::Enabled]
        .iter()
        .all(|state| node.states.contains(state))
}

/// Whether the name starts with the action's word ("New Tab" for "new").
fn names(node: &A11yNode, word: &str) -> bool {
    node.name
        .value
        .split_whitespace()
        .next()
        .is_some_and(|first| first.eq_ignore_ascii_case(word))
}

/// The first visible, enabled control in `nodes` whose meaning is the same as the chord of a
/// `Key` action; `None` for any other action and for no tree (`nodes` empty).
pub fn control_for(action: &CuaAction<WindowSpace>, nodes: &[A11yNode]) -> Option<Control> {
    let CuaAction::Key { chord, .. } = action else {
        return None;
    };
    if chord.mods.is_empty() {
        return None;
    }
    let meant: Vec<String> = standard_actions_of(chord)
        .into_iter()
        .filter(|a| GUARDED.contains(a))
        .map(StandardAction::label)
        .collect();
    nodes
        .iter()
        .filter(|n| CLICKABLE.contains(&n.role) && is_shown(n))
        .find(|n| meant.iter().any(|word| names(n, word)))
        .map(|n| Control {
            id: n.id,
            name: n.name.value.clone(),
        })
}
