//! How much an action matters, before the router sees it.

use a11y_tree::A11yNode;
use cua_action::{CuaAction, WindowSpace};
use docket_core::{EffectBasis, WindowClass};
use prov::Effect;

/// The effect of `action`, and why: a node that maps to a typed action wins (the basis names the
/// action and the router replaces the effect with the one the action declares; the effect
/// returned is the default table's); otherwise a window class makes typing and Enter `Outbound`
/// in a mail compose window, a terminal or a payments window; otherwise the default table
/// (observing is `Read`, pointer actions and keys `UndoableWrite`). The router makes the ruling;
/// this only names the effect it is asked about. Pure.
pub fn classify_effect(
    action: &CuaAction<WindowSpace>,
    node: Option<&A11yNode>,
    class: WindowClass,
) -> (Effect, EffectBasis) {
    let _ = (action, node, class);
    todo!(
        "classify_effect_table: node typed action first, then window class, then the default table"
    )
}
