//! The controls that commit a send, a submission, a payment or a transfer.
//!
//! A closed set read off the accessibility role and name the capture gives, and nothing else: a
//! node is a commit control when its role is button-like and one word of its name is in the
//! rules ([`CommitRules`]). Words are the name lower-cased and split at every character that is
//! not a letter or a digit, so "Pay now" and "Send & close" match and "Payroll" and "Sender" do
//! not. The table errs towards asking: "Confirm" and "Order" count.

use crate::rules::CommitRules;
use a11y_tree::{A11yNode, Role};

/// Roles that are pressed to act.
fn presses(role: Role) -> bool {
    matches!(
        role,
        Role::Button | Role::ToggleButton | Role::MenuItem | Role::Link
    )
}

/// What the first committing word of `node`'s name commits: `Some(true)` for a payment,
/// `Some(false)` for any other commit, `None` when the node is not a commit control.
fn first_commit(rules: &CommitRules, node: &A11yNode) -> Option<bool> {
    if !presses(node.role) {
        return None;
    }
    let name = node.name.value.to_lowercase();
    name.split(|c: char| !c.is_alphanumeric()).find_map(|word| {
        if rules.pay.iter().any(|w| w == word) {
            Some(true)
        } else {
            rules.also.iter().any(|w| w == word).then_some(false)
        }
    })
}

/// Whether `node` is a commit control.
pub fn commits(rules: &CommitRules, node: &A11yNode) -> bool {
    first_commit(rules, node).is_some()
}

/// Whether `node` is a commit control whose first committing word pays.
pub fn pays(rules: &CommitRules, node: &A11yNode) -> bool {
    first_commit(rules, node) == Some(true)
}
