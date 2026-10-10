//! The snapshot of one window's accessibility tree.

use cua_action::{NodeId, Rect, WindowSpace};
use cua_desktop::Stamp;
pub use cua_types::Role;
use porter_core::Count;
use prov::{ActionName, Labelled};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Where a tree came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeSource {
    /// AT-SPI.
    Atspi,
    /// A quire (Blitz) app: `accesskit_unix`, still AT-SPI on the wire, plus typed hints.
    QuireApp,
}

/// Whether pruning dropped nodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Truncation {
    /// Everything is here.
    Complete,
    /// This many nodes were dropped to fit the budget.
    Pruned {
        /// How many.
        dropped: Count,
    },
}

/// A window's tree. The nodes are an arena; `nodes[0]` is the window root.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct A11ySnapshot {
    /// Where it came from.
    pub source: TreeSource,
    /// When it was taken.
    pub taken: Stamp,
    /// The nodes.
    pub nodes: Vec<A11yNode>,
    /// Whether it was pruned.
    pub truncated: Truncation,
}

/// A node's parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Parent {
    /// The window root has none.
    Root,
    /// Another node.
    Node(NodeId),
}

/// A node's value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum NodeValue {
    /// None.
    None,
    /// Text: screen content, labelled untrusted.
    Text(Labelled<String>),
    /// A number in a range.
    Range {
        /// Now.
        now: i64,
        /// The least.
        min: i64,
        /// The most.
        max: i64,
    },
}

/// One state bit of a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeState {
    /// Enabled.
    Enabled,
    /// Can take focus.
    Focusable,
    /// Has focus.
    Focused,
    /// Selected.
    Selected,
    /// Checked.
    Checked,
    /// Expanded.
    Expanded,
    /// Its text can be edited.
    Editable,
    /// Visible.
    Visible,
    /// Showing on screen.
    Showing,
}

/// An action a node offers (an AT-SPI action name, parsed).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum NodeAction {
    /// `click`.
    Click,
    /// `press`.
    Press,
    /// `activate`.
    Activate,
    /// `toggle`.
    Toggle,
    /// `expand`.
    Expand,
    /// An action this list does not name.
    Other(String),
}

/// The typed action a quire widget maps to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TypedHint {
    /// None.
    None,
    /// This action.
    Action(ActionName),
}

/// Whether a node's content may reach a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    /// Ordinary.
    Normal,
    /// A password entry or a quire-declared sensitive control.
    Secret,
}

/// How many nodes a snapshot keeps (setting `cua.tree.max_nodes`, default 400).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NodeBudget(pub u32);

/// One node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct A11yNode {
    /// Stable within one run.
    pub id: NodeId,
    /// Its parent.
    pub parent: Parent,
    /// What it is.
    pub role: Role,
    /// Its name: screen text, labelled `Untrusted(Screen)`.
    pub name: Labelled<String>,
    /// Its value.
    pub value: NodeValue,
    /// Its state bits.
    pub states: BTreeSet<NodeState>,
    /// Where it is, window-local logical pixels.
    pub bounds: Rect<WindowSpace>,
    /// What it offers.
    pub actions: Vec<NodeAction>,
    /// The typed action a quire widget maps it to.
    pub typed: TypedHint,
    /// Whether its content may reach a model.
    pub sensitivity: Sensitivity,
}
