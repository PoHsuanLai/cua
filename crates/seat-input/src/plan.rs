//! From an action to input operations. Pure.

use crate::caps::SeatCaps;
use crate::keymap::Keymap;
use crate::op::InputOp;
use cua_action::{CuaAction, Point, Size, WindowSpace};
use serde::{Deserialize, Serialize};

/// The points an action targets, with accessibility nodes already resolved by the caller to the
/// centre of their bounds, clamped inside the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ResolvedPoints {
    /// The action targets nothing (a key, a wait).
    None,
    /// One point (click, move, scroll).
    One(Point<WindowSpace>),
    /// A drag.
    Drag {
        /// Where it starts.
        from: Point<WindowSpace>,
        /// Where it ends.
        to: Point<WindowSpace>,
    },
}

/// One action, ready to plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedAction {
    /// The action.
    pub action: CuaAction<WindowSpace>,
    /// Its targets.
    pub at: ResolvedPoints,
    /// The window, for the bounds check.
    pub window: Size<WindowSpace>,
}

/// Why an action cannot become input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum PlanError {
    /// A target outside the window.
    #[error("point outside the window")]
    OutsideWindow(Point<WindowSpace>),
    /// A character no text path can type (`你` with the key-code path only).
    #[error("cannot type {0:?} here")]
    Unmappable(char),
    /// The action is not input (`Finish`, `Ask`, `Zoom`, `Wait`, `Observe`).
    #[error("not an input action")]
    NotAnInput,
    /// A chord key the keymap does not have.
    #[error("unknown key {0}")]
    UnknownKey(String),
}

/// The operations that perform `action`: a click is move, down, gap, up (repeated for double and
/// triple), a drag is move, down, moves, up, a chord is modifiers down, key down and up (repeated),
/// modifiers up. Refuses a point outside the window. Pure.
pub fn plan(
    action: &PlannedAction,
    keymap: &Keymap,
    caps: &SeatCaps,
) -> Result<Vec<InputOp>, PlanError> {
    let _ = (action, keymap, caps);
    todo!(
        "plan_click_double_right_mods_table, plan_drag_emits_down_moves_up, plan_rejects_point_outside_window, unmappable_char_errors_named"
    )
}
