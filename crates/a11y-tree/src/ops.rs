//! What a snapshot is used for. All pure.

use crate::model::{A11ySnapshot, NodeBudget};
use crate::selector::{NodeSelector, Resolution};
use cua_action::{NodeId, Rect, WindowSpace};
use serde::{Deserialize, Serialize};

/// What changed between two snapshots of one window, by node id.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TreeDiff {
    /// Ids only the later one has.
    pub added: Vec<NodeId>,
    /// Ids only the earlier one has.
    pub removed: Vec<NodeId>,
    /// Ids both have whose node differs.
    pub changed: Vec<NodeId>,
}

/// The snapshot cut to `budget` nodes: visible and actionable nodes first, ancestors of what
/// is kept always kept, the rest dropped and counted in `truncated`.
pub fn prune(snapshot: &A11ySnapshot, budget: NodeBudget) -> A11ySnapshot {
    let _ = (snapshot, budget);
    todo!("prune_keeps_actionable_visible_first_within_budget")
}

/// The tree as the model reads it, one node per line:
/// `[12] button "Save" enabled,focusable @340,20 80x24`. A secret node's value is never
/// rendered. Stable: the same snapshot always renders the same text.
pub fn render(snapshot: &A11ySnapshot) -> String {
    let _ = snapshot;
    todo!(
        "render_is_stable_golden over fixtures/a11y/*.json; turn_renders_tree_without_secret_values"
    )
}

/// What changed from `before` to `after`.
pub fn diff(before: &A11ySnapshot, after: &A11ySnapshot) -> TreeDiff {
    let _ = (before, after);
    todo!("diff_reports_added_removed_changed")
}

/// The regions of secret nodes, window-local, as mask hints for `maskcap`.
pub fn secret_rects(snapshot: &A11ySnapshot) -> Vec<Rect<WindowSpace>> {
    let _ = snapshot;
    todo!("secret_rects_cover_password_text")
}

/// Which nodes `selector` names.
pub fn resolve(snapshot: &A11ySnapshot, selector: &NodeSelector) -> Resolution {
    let _ = (snapshot, selector);
    todo!("selector_resolution_table (procedures, a later wave)")
}
