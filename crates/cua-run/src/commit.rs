//! The controls that commit a send, a submission, a payment or a transfer.
//!
//! A closed set read off the accessibility role and name the capture gives, and nothing else: a
//! node is a commit control when its role is button-like and one word of its name is in
//! [`WORDS`]. Words are the name lower-cased and split at every character that is not a letter
//! or a digit, so "Pay now" and "Send & close" match and "Payroll" and "Sender" do not. The
//! table errs towards asking: "Confirm" and "Order" count.

use a11y_tree::{A11yNode, Role};

/// What a commit control commits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Commit {
    /// A message or a form leaves.
    Send,
    /// A form is submitted.
    Submit,
    /// Money is paid.
    Pay,
    /// Money is moved.
    Transfer,
    /// A pending step is confirmed.
    Confirm,
}

/// Words of a control's name, lower case, and what each commits.
const WORDS: &[(&str, Commit)] = &[
    ("send", Commit::Send),
    ("submit", Commit::Submit),
    ("pay", Commit::Pay),
    ("payment", Commit::Pay),
    ("purchase", Commit::Pay),
    ("buy", Commit::Pay),
    ("order", Commit::Pay),
    ("donate", Commit::Pay),
    ("transfer", Commit::Transfer),
    ("confirm", Commit::Confirm),
    ("authorize", Commit::Confirm),
    ("authorise", Commit::Confirm),
];

/// Roles that are pressed to act.
fn presses(role: Role) -> bool {
    matches!(
        role,
        Role::Button | Role::ToggleButton | Role::MenuItem | Role::Link
    )
}

/// What `node` commits, if it is a commit control.
pub fn commit_of(node: &A11yNode) -> Option<Commit> {
    if !presses(node.role) {
        return None;
    }
    let name = node.name.value.to_lowercase();
    name.split(|c: char| !c.is_alphanumeric())
        .find_map(|word| WORDS.iter().find(|(w, _)| *w == word).map(|(_, c)| *c))
}
