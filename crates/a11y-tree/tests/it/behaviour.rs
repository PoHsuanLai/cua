//! What the tree functions must do, over the fixtures.

use a11y_tree::{
    NameMatch, NodeBudget, NodeSelector, Resolution, Role, SelectorStep, Truncation, diff, prune,
    resolve, secret_rects,
};
use cua_action::NodeId;

#[test]
fn prune_keeps_actionable_visible_first_within_budget() {
    let form = crate::support::firefox_form();
    let total = form.nodes.len();
    let pruned = prune(&form, NodeBudget(5));
    assert_eq!(pruned.nodes.len(), 5);
    assert_eq!(pruned.nodes[0].role, Role::Document, "the root stays first");
    let dropped = u32::try_from(total - 5).unwrap();
    assert_eq!(
        pruned.truncated,
        Truncation::Pruned {
            dropped: porter_core::Count(dropped)
        }
    );
    assert!(
        pruned.nodes.iter().any(|n| n.role == Role::Button),
        "the sign-in button is actionable and visible, so it survives"
    );
    let all = prune(&form, NodeBudget(400));
    assert_eq!(all, form, "a budget that fits changes nothing");
}

#[test]
fn secret_rects_cover_password_text() {
    let form = crate::support::firefox_form();
    let password = form
        .nodes
        .iter()
        .find(|n| n.role == Role::PasswordText)
        .expect("password");
    assert_eq!(secret_rects(&form), vec![password.bounds]);
    assert!(secret_rects(&crate::support::gtk4_dialog()).is_empty());
}

#[test]
fn diff_reports_added_removed_changed() {
    let before = crate::support::gtk4_dialog();
    let mut after = before.clone();
    let removed = after.nodes.pop().expect("a node");
    after.nodes[2].states.insert(a11y_tree::NodeState::Focused);
    let d = diff(&before, &after);
    assert_eq!(d.removed, vec![removed.id]);
    assert_eq!(d.changed, vec![NodeId(2)]);
    assert!(d.added.is_empty());
    assert_eq!(diff(&before, &before), a11y_tree::TreeDiff::default());
}

#[test]
fn selector_resolution_table() {
    let dialog = crate::support::gtk4_dialog();
    let step = |role, name: &str| SelectorStep {
        role,
        name: NameMatch::Exact(name.into()),
    };
    let one = NodeSelector {
        path: vec![
            step(Role::Dialog, "Save changes?"),
            step(Role::Button, "Save"),
        ],
    };
    assert_eq!(resolve(&dialog, &one), Resolution::One(NodeId(4)));
    let any = NodeSelector {
        path: vec![
            SelectorStep {
                role: Role::Dialog,
                name: NameMatch::Any,
            },
            SelectorStep {
                role: Role::Button,
                name: NameMatch::Any,
            },
        ],
    };
    assert_eq!(
        resolve(&dialog, &any),
        Resolution::Many(vec![NodeId(2), NodeId(3), NodeId(4)])
    );
    let none = NodeSelector {
        path: vec![step(Role::Dialog, "Other")],
    };
    assert_eq!(resolve(&dialog, &none), Resolution::None);
}
