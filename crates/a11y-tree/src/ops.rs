//! What a snapshot is used for. All pure. (`render` is in its own module.)

use crate::model::{A11yNode, A11ySnapshot, NodeBudget, NodeState, Parent, Truncation};
use crate::render::is_secret;
use crate::selector::{NameMatch, NodeSelector, Resolution, SelectorStep};
use cua_action::{NodeId, Rect, WindowSpace};
use porter_core::Count;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub use crate::render::render;

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

/// How much a node is worth keeping: an actionable node on screen first, then anything on
/// screen, then the rest.
fn rank(node: &A11yNode) -> u8 {
    let on_screen =
        node.states.contains(&NodeState::Visible) && node.states.contains(&NodeState::Showing);
    let actionable = !node.actions.is_empty()
        || node.states.contains(&NodeState::Focusable)
        || node.states.contains(&NodeState::Editable);
    match (on_screen, actionable) {
        (true, true) => 0,
        (true, false) => 1,
        (false, _) => 2,
    }
}

/// The indexes of `at` and of every ancestor of it that is not yet in `kept`, nearest first.
fn missing_with_ancestors(
    snapshot: &A11ySnapshot,
    by_id: &BTreeMap<NodeId, usize>,
    kept: &BTreeSet<usize>,
    at: usize,
) -> Vec<usize> {
    let mut need = Vec::new();
    let mut here = at;
    for _ in 0..=snapshot.nodes.len() {
        if kept.contains(&here) || need.contains(&here) {
            break;
        }
        need.push(here);
        match snapshot.nodes[here].parent {
            Parent::Node(up) => match by_id.get(&up) {
                Some(index) => here = *index,
                None => break,
            },
            Parent::Root => break,
        }
    }
    need
}

/// The snapshot cut to `budget` nodes: the root first, then actionable nodes on screen, then
/// anything on screen, then the rest (each class in tree order); a node is only kept with all
/// its ancestors, so the kept nodes are still a tree. The kept nodes keep their order, the
/// dropped ones are counted in `truncated` (added to what an earlier pruning dropped). A budget
/// that already fits changes nothing; a budget of zero keeps the root alone.
pub fn prune(snapshot: &A11ySnapshot, budget: NodeBudget) -> A11ySnapshot {
    let limit = usize::try_from(budget.0).unwrap_or(usize::MAX).max(1);
    if snapshot.nodes.len() <= limit {
        return snapshot.clone();
    }
    let by_id: BTreeMap<NodeId, usize> = snapshot
        .nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.id, i))
        .collect();
    let mut order: Vec<usize> = (0..snapshot.nodes.len()).collect();
    order.sort_by_key(|&i| (i != 0, rank(&snapshot.nodes[i]), i));
    let mut kept: BTreeSet<usize> = BTreeSet::new();
    for at in order {
        let need = missing_with_ancestors(snapshot, &by_id, &kept, at);
        if kept.len() + need.len() <= limit {
            kept.extend(need);
        }
        if kept.len() == limit {
            break;
        }
    }
    let nodes: Vec<A11yNode> = kept.iter().map(|&i| snapshot.nodes[i].clone()).collect();
    let before = match snapshot.truncated {
        Truncation::Complete => 0,
        Truncation::Pruned { dropped } => dropped.0,
    };
    let now = u32::try_from(snapshot.nodes.len() - nodes.len()).unwrap_or(u32::MAX);
    A11ySnapshot {
        nodes,
        truncated: Truncation::Pruned {
            dropped: Count(before.saturating_add(now)),
        },
        ..snapshot.clone()
    }
}

/// What changed from `before` to `after`, by node id, each list in the order of the snapshot it
/// comes from.
pub fn diff(before: &A11ySnapshot, after: &A11ySnapshot) -> TreeDiff {
    let old: BTreeMap<NodeId, &A11yNode> = before.nodes.iter().map(|n| (n.id, n)).collect();
    let new: BTreeMap<NodeId, &A11yNode> = after.nodes.iter().map(|n| (n.id, n)).collect();
    TreeDiff {
        added: after
            .nodes
            .iter()
            .filter(|n| !old.contains_key(&n.id))
            .map(|n| n.id)
            .collect(),
        removed: before
            .nodes
            .iter()
            .filter(|n| !new.contains_key(&n.id))
            .map(|n| n.id)
            .collect(),
        changed: after
            .nodes
            .iter()
            .filter(|n| old.get(&n.id).is_some_and(|o| *o != *n))
            .map(|n| n.id)
            .collect(),
    }
}

/// The regions of secret nodes (marked secret, or a password entry), window-local, as mask
/// hints for `maskcap`, in tree order.
pub fn secret_rects(snapshot: &A11ySnapshot) -> Vec<Rect<WindowSpace>> {
    secret_rects_of(&snapshot.nodes)
}

/// [`secret_rects`] over any nodes, such as a tree's nodes together with the ones a caller took
/// out of it.
pub fn secret_rects_of<'a>(
    nodes: impl IntoIterator<Item = &'a A11yNode>,
) -> Vec<Rect<WindowSpace>> {
    nodes
        .into_iter()
        .filter(|n| is_secret(n))
        .map(|n| n.bounds)
        .collect()
}

fn step_matches(step: &SelectorStep, node: &A11yNode) -> bool {
    step.role == node.role
        && match &step.name {
            NameMatch::Any => true,
            NameMatch::Exact(name) => node.name.value == *name,
            // A slot is filled at replay, and a snapshot carries no slots: it names nothing.
            NameMatch::Slot(_) => false,
        }
}

/// Which nodes `selector` names: the first step must match the window root, each later step a
/// child of what the one before matched. An empty path names nothing. Slots name nothing here
/// (their values arrive at replay).
pub fn resolve(snapshot: &A11ySnapshot, selector: &NodeSelector) -> Resolution {
    let Some((first, rest)) = selector.path.split_first() else {
        return Resolution::None;
    };
    let mut at: Vec<NodeId> = snapshot
        .nodes
        .iter()
        .filter(|n| n.parent == Parent::Root && step_matches(first, n))
        .map(|n| n.id)
        .collect();
    for step in rest {
        at = snapshot
            .nodes
            .iter()
            .filter(|n| matches!(n.parent, Parent::Node(up) if at.contains(&up)))
            .filter(|n| step_matches(step, n))
            .map(|n| n.id)
            .collect();
    }
    match at.as_slice() {
        [] => Resolution::None,
        [one] => Resolution::One(*one),
        _ => Resolution::Many(at),
    }
}
