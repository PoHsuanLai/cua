//! A surface as an accessibility tree: the role, name and sensitivity of what the window shows.
//!
//! The tree is what the run reads to rule on a window (a password entry makes it sensitive
//! whatever its title says). An item says what it is to the tree with `role` and `sensitivity`;
//! when it says nothing, its kind does.
//!
//! The bounds are empty on purpose. The eval aims by pixels and classes a click by the pixels
//! it lands on, as it did before a tree existed; a node with room would take clicks away from
//! the pixel rules and change what the gate is asked. The eval's models never see this tree
//! either (`run::Timed` drops it from their request).

use super::def::{AxRole, Kind};
use super::state::{Placed, Surface};
use a11y_tree::{
    A11yNode, A11ySnapshot, NodeAction, NodeState, NodeValue, Parent, Role, Sensitivity,
    TreeSource, Truncation, TypedHint,
};
use cua_action::{Coord, NodeId, Point, Rect, Size};
use cua_desktop::Stamp;
use prov::{Label, Labelled};
use std::collections::BTreeSet;

/// The id of the window node; the items follow it.
const ROOT: NodeId = NodeId(0);

impl From<AxRole> for Role {
    fn from(role: AxRole) -> Role {
        match role {
            AxRole::Text => Role::Text,
            AxRole::Label => Role::Label,
            AxRole::Heading => Role::Heading,
            AxRole::Button => Role::Button,
            AxRole::Link => Role::Link,
            AxRole::ListItem => Role::ListItem,
            AxRole::CheckBox => Role::CheckBox,
            AxRole::Entry => Role::Entry,
            AxRole::PasswordText => Role::PasswordText,
        }
    }
}

fn role_of(placed: &Placed<'_>) -> Role {
    if let Some(role) = placed.item.role {
        return role.into();
    }
    match placed.item.kind {
        Kind::Label => Role::Label,
        Kind::Button => Role::Button,
        Kind::Row => Role::ListItem,
        Kind::Field => Role::Entry,
        Kind::Check => Role::CheckBox,
    }
}

fn empty_at(x: u32, y: u32) -> Rect<cua_action::WindowSpace> {
    Rect {
        origin: Point::new(Coord(x), Coord(y)),
        size: Size::new(Coord(0), Coord(0)),
    }
}

fn shown() -> BTreeSet<NodeState> {
    BTreeSet::from([NodeState::Enabled, NodeState::Visible, NodeState::Showing])
}

fn node_of(placed: &Placed<'_>, focused: bool, label: &Label) -> A11yNode {
    let kind = placed.item.kind;
    let mut states = shown();
    if kind != Kind::Label {
        states.insert(NodeState::Focusable);
    }
    if kind == Kind::Field {
        states.insert(NodeState::Editable);
    }
    if focused {
        states.insert(NodeState::Focused);
    }
    let actions = match kind {
        Kind::Label | Kind::Field => vec![],
        Kind::Button | Kind::Row | Kind::Check => vec![NodeAction::Click],
    };
    A11yNode {
        id: NodeId(u32::try_from(placed.index).map_or(u32::MAX, |i| i.saturating_add(1))),
        parent: Parent::Node(ROOT),
        role: role_of(placed),
        name: Labelled::new(placed.text.clone(), label.clone()),
        value: NodeValue::None,
        states,
        bounds: empty_at(placed.rect[0], placed.rect[1]),
        actions,
        typed: TypedHint::None,
        sensitivity: placed.item.sensitivity.unwrap_or(Sensitivity::Normal),
    }
}

/// The tree of `surface` as it shows now, taken at `taken`. Names are screen text and carry
/// `label`.
pub fn snapshot(surface: &Surface, taken: Stamp, label: &Label) -> A11ySnapshot {
    let focused = surface.focused().map(str::to_owned);
    let window = A11yNode {
        id: ROOT,
        parent: Parent::Root,
        role: Role::Window,
        name: Labelled::new(surface.title(), label.clone()),
        value: NodeValue::None,
        states: shown(),
        bounds: empty_at(0, 0),
        actions: vec![],
        typed: TypedHint::None,
        sensitivity: Sensitivity::Normal,
    };
    let items = surface.visible().into_iter().map(|placed| {
        let has_focus =
            !placed.item.id.is_empty() && focused.as_deref() == Some(placed.item.id.as_str());
        node_of(&placed, has_focus, label)
    });
    A11ySnapshot {
        source: TreeSource::Atspi,
        taken,
        nodes: std::iter::once(window).chain(items).collect(),
        truncated: Truncation::Complete,
    }
}
