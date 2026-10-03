//! The tree as the model reads it.

use crate::model::{
    A11yNode, A11ySnapshot, NodeState, NodeValue, Parent, Role, Sensitivity, Truncation, TypedHint,
};
use cua_action::NodeId;
use std::collections::BTreeMap;
use std::fmt::Write;

/// The word a role is written as in the rendered tree and in the router's `NodeFacts`: its snake_case slug, and `other(<number>)` for a raw role.
pub fn role_word(role: Role) -> String {
    match role {
        Role::Window => "window".to_owned(),
        Role::Dialog => "dialog".to_owned(),
        Role::Button => "button".to_owned(),
        Role::ToggleButton => "toggle_button".to_owned(),
        Role::CheckBox => "check_box".to_owned(),
        Role::RadioButton => "radio_button".to_owned(),
        Role::ComboBox => "combo_box".to_owned(),
        Role::MenuBar => "menu_bar".to_owned(),
        Role::Menu => "menu".to_owned(),
        Role::MenuItem => "menu_item".to_owned(),
        Role::Tab => "tab".to_owned(),
        Role::TabList => "tab_list".to_owned(),
        Role::Link => "link".to_owned(),
        Role::Entry => "entry".to_owned(),
        Role::PasswordText => "password_text".to_owned(),
        Role::Text => "text".to_owned(),
        Role::Label => "label".to_owned(),
        Role::Heading => "heading".to_owned(),
        Role::List => "list".to_owned(),
        Role::ListItem => "list_item".to_owned(),
        Role::Tree => "tree".to_owned(),
        Role::TreeItem => "tree_item".to_owned(),
        Role::Table => "table".to_owned(),
        Role::Cell => "cell".to_owned(),
        Role::Slider => "slider".to_owned(),
        Role::SpinButton => "spin_button".to_owned(),
        Role::ScrollBar => "scroll_bar".to_owned(),
        Role::ProgressBar => "progress_bar".to_owned(),
        Role::Image => "image".to_owned(),
        Role::ToolBar => "tool_bar".to_owned(),
        Role::Document => "document".to_owned(),
        Role::Section => "section".to_owned(),
        Role::Other(raw) => format!("other({raw})"),
    }
}

fn state_word(state: NodeState) -> &'static str {
    match state {
        NodeState::Enabled => "enabled",
        NodeState::Focusable => "focusable",
        NodeState::Focused => "focused",
        NodeState::Selected => "selected",
        NodeState::Checked => "checked",
        NodeState::Expanded => "expanded",
        NodeState::Editable => "editable",
        NodeState::Visible => "visible",
        NodeState::Showing => "showing",
    }
}

/// Screen text as one quoted token: quotes, backslashes and every control character are escaped,
/// so a name can never end its own line and pass for another node.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{{{:x}}}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A node whose content never reaches a model: marked secret, or a password entry whatever
/// the source said.
pub(crate) fn is_secret(node: &A11yNode) -> bool {
    node.sensitivity == Sensitivity::Secret || node.role == Role::PasswordText
}

fn value_text(node: &A11yNode) -> Option<String> {
    match (&node.value, is_secret(node)) {
        (NodeValue::None, _) => None,
        (_, true) => Some("<hidden>".to_owned()),
        (NodeValue::Text(text), false) => Some(quoted(&text.value)),
        (NodeValue::Range { now, min, max }, false) => Some(format!("{now} ({min}..{max})")),
    }
}

fn line(node: &A11yNode, depth: usize) -> String {
    let mut out = format!(
        "{}[{}] {} {}",
        "  ".repeat(depth),
        node.id.0,
        role_word(node.role),
        quoted(&node.name.value),
    );
    if !node.states.is_empty() {
        let states: Vec<&str> = node.states.iter().map(|s| state_word(*s)).collect();
        out.push(' ');
        out.push_str(&states.join(","));
    }
    let b = node.bounds;
    let _ = write!(
        out,
        " @{},{} {}x{}",
        b.origin.x.0, b.origin.y.0, b.size.w.0, b.size.h.0
    );
    if let Some(value) = value_text(node) {
        let _ = write!(out, " = {value}");
    }
    if let TypedHint::Action(action) = &node.typed {
        let _ = write!(out, " -> {}", action.as_str());
    }
    out
}

/// How deep each node sits, by following parents; a parent that is missing or a loop ends the
/// climb, so a malformed arena still renders.
fn depths(snapshot: &A11ySnapshot) -> BTreeMap<NodeId, usize> {
    let parents: BTreeMap<NodeId, Parent> =
        snapshot.nodes.iter().map(|n| (n.id, n.parent)).collect();
    snapshot
        .nodes
        .iter()
        .map(|node| {
            let mut depth = 0;
            let mut at = node.parent;
            while let Parent::Node(up) = at {
                if depth > snapshot.nodes.len() || !parents.contains_key(&up) {
                    break;
                }
                depth += 1;
                at = parents[&up];
            }
            (node.id, depth)
        })
        .collect()
}

/// The tree as the model reads it, one node per line, children indented under their parent:
/// `[12] button "Save" enabled,focusable @340,20 80x24`, then ` = <value>` for a node with a
/// value (`<hidden>` for a secret one: its value is never rendered) and ` -> <action>` for a
/// typed hint. A pruned tree ends with a line that says how many nodes are not shown. Stable:
/// the same snapshot always renders the same text.
pub fn render(snapshot: &A11ySnapshot) -> String {
    let depth = depths(snapshot);
    let mut lines: Vec<String> = snapshot
        .nodes
        .iter()
        .map(|node| line(node, depth.get(&node.id).copied().unwrap_or(0)))
        .collect();
    if let Truncation::Pruned { dropped } = snapshot.truncated {
        lines.push(format!("... {} more nodes not shown", dropped.0));
    }
    lines.join("\n")
}
