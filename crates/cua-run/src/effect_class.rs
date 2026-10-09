//! How much an action matters, before the router sees it.

use crate::commit::commit_of;
use a11y_tree::{A11yNode, TypedHint};
use cua_action::{ActionClass, CuaAction, WindowSpace};
use docket_core::{EffectBasis, WindowClass};
use keyboard_types::Key;
use prov::Effect;

/// The effect of `action`, and why: a node that maps to a typed action wins (the basis names the
/// action and the router replaces the effect with the one the action declares; the effect
/// returned is the default table's); otherwise a window class makes typing and Enter `Outbound`
/// in a mail compose window, a terminal, a payments window or a bank window, and so does a click on a commit control (Send, Pay, Submit,
/// Transfer, Confirm) in a mail compose, payments or bank window, or a click there on a node that
/// is not known; otherwise the default table
/// (observing is `Read`, pointer actions and keys `UndoableWrite`). The router makes the ruling;
/// this only names the effect it is asked about. Pure.
pub fn classify_effect(
    action: &CuaAction<WindowSpace>,
    node: Option<&A11yNode>,
    class: WindowClass,
) -> (Effect, EffectBasis) {
    let default = match action.class() {
        ActionClass::Observe | ActionClass::Conclude => {
            return (Effect::Read, EffectBasis::DefaultTable);
        }
        ActionClass::Pointer | ActionClass::Keyboard => Effect::UndoableWrite,
    };
    if let Some(TypedHint::Action(name)) = node.map(|n| &n.typed) {
        return (default, EffectBasis::NodeTypedAction(name.clone()));
    }
    if sends_something(action) && sends_in(class) {
        return (Effect::Outbound, EffectBasis::WindowClass(class));
    }
    if commits_in(action, node, class) {
        return (Effect::Outbound, EffectBasis::WindowClass(class));
    }
    (default, EffectBasis::DefaultTable)
}

/// A click in a form window (mail compose, payments, banking) on a commit control
/// ([`commit_of`]). A click whose node is unknown (no tree, or nothing under the point) might be
/// the commit, so it counts: the window asks rather than guesses.
fn commits_in(
    action: &CuaAction<WindowSpace>,
    node: Option<&A11yNode>,
    class: WindowClass,
) -> bool {
    let form = matches!(
        class,
        WindowClass::MailCompose | WindowClass::Payments | WindowClass::Banking
    );
    let click = matches!(action, CuaAction::Click { .. });
    form && click && node.is_none_or(|n| commit_of(n).is_some())
}

/// Typing, or a chord whose key is Enter: what puts typed text on its way.
fn sends_something(action: &CuaAction<WindowSpace>) -> bool {
    match action {
        CuaAction::Type { .. } => true,
        CuaAction::Key { chord, .. } => chord.key == Key::Enter,
        _ => false,
    }
}

/// The windows where typing and Enter leave the machine: a mail compose window, a terminal, a
/// payment form, a bank.
fn sends_in(class: WindowClass) -> bool {
    matches!(
        class,
        WindowClass::MailCompose
            | WindowClass::Terminal
            | WindowClass::Payments
            | WindowClass::Banking
    )
}
