//! Text the tree carries and the person cannot see.
//!
//! A node can hold words that are not on the screen: it has no size, it stands to the right of
//! the window, or the tree says it is neither visible nor showing. Browsers always carry such
//! nodes (background tabs' documents, zero-size labels), and the model would read them as if the
//! person saw them. So [`seen_snapshot`] leaves them out of the tree the model reads, whatever
//! their word count, and re-parents their children; the check reports an unseen node only when
//! it also holds an instruction or hidden characters.
//!
//! What is not judged, on purpose: a node below the bottom edge (a long page scrolled to its
//! top is full of them), a node whose bounds are unknown (a tree where no node has any extent has
//! no geometry to judge), and a tree that sets no state bits at all. The window root stays.

use a11y_tree::{A11yNode, A11ySnapshot, NodeState, Parent, Truncation, secret_rects_of};
use cua_action::{NodeId, Rect, Size, WindowSpace};
use porter_ids::Count;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A side shorter than this many logical pixels is no room for text.
const TINY_SIDE: u32 = 3;

/// Why the person cannot see a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnseenWhy {
    /// Its bounds are zero or tiny.
    NoSize,
    /// It stands to the right of the window's edge.
    OffWindow,
    /// The tree marks it as neither visible nor showing.
    NotShown,
}

/// Whether the tree has geometry to judge by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Geometry {
    /// Some node has extent: bounds mean something. Carries the window's size.
    Known(Size<WindowSpace>),
    /// No node has any extent: the source gave no geometry.
    Absent,
}

impl Geometry {
    /// The geometry of `nodes` in a window of `window`.
    pub(super) fn of(nodes: &[A11yNode], window: Size<WindowSpace>) -> Geometry {
        let has_extent = |n: &A11yNode| n.bounds.size.w.0 > 0 && n.bounds.size.h.0 > 0;
        if nodes.iter().any(has_extent) {
            Geometry::Known(window)
        } else {
            Geometry::Absent
        }
    }
}

/// Why `node` cannot be seen, if it cannot.
pub(super) fn why_unseen(node: &A11yNode, geometry: Geometry) -> Option<UnseenWhy> {
    if let Geometry::Known(window) = geometry {
        let size = node.bounds.size;
        if size.w.0 < TINY_SIDE || size.h.0 < TINY_SIDE {
            return Some(UnseenWhy::NoSize);
        }
        if window.w.0 > 0 && node.bounds.origin.x.0 >= window.w.0 {
            return Some(UnseenWhy::OffWindow);
        }
    }
    let marked = !node.states.is_empty()
        && !node.states.contains(&NodeState::Visible)
        && !node.states.contains(&NodeState::Showing);
    marked.then_some(UnseenWhy::NotShown)
}

/// The nearest ancestor (or the root) of a node whose parent is `at` that is not in `unseen`.
fn nearest_seen(
    mut at: Parent,
    unseen: &BTreeSet<NodeId>,
    parents: &BTreeMap<NodeId, Parent>,
    limit: usize,
) -> Parent {
    for _ in 0..=limit {
        match at {
            Parent::Node(up) if unseen.contains(&up) => {
                at = parents.get(&up).copied().unwrap_or(Parent::Root);
            }
            Parent::Node(_) | Parent::Root => break,
        }
    }
    at
}

/// The nodes taken out of the tree the model reads, with why. Only the screen check reads them
/// (to find an instruction hidden in one); it is never serialised, so the words go nowhere else.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Withheld(pub Vec<(UnseenWhy, A11yNode)>);

impl Withheld {
    /// How many nodes were taken out.
    pub fn count(&self) -> usize {
        self.0.len()
    }
}

/// The rectangles to mask for the secret nodes of a tree, the ones the model reads and the ones
/// withheld from it alike. Withholding a node never takes its mask away: a toolkit can drop
/// SHOWING on a password field that is on screen, or a field can stand partly off the window.
pub fn secret_masks(seen: &A11ySnapshot, withheld: &Withheld) -> Vec<Rect<WindowSpace>> {
    secret_rects_of(
        seen.nodes
            .iter()
            .chain(withheld.0.iter().map(|(_, node)| node)),
    )
}

/// The snapshot without the nodes the person cannot see, for a window of `window`, and those
/// nodes. This is the one place the model's view of the tree is made: every later reader (the
/// rendered tree, `Target::Node` resolution, the step record, the reader's text) takes the
/// pruned tree. Node ids are not renumbered, so every id left is the one the source gave; the
/// children of a dropped node move up to its nearest kept ancestor. The dropped nodes are
/// counted in `truncated`. The window root (`nodes[0]`) stays.
pub fn split_unseen(snapshot: A11ySnapshot, window: Size<WindowSpace>) -> (A11ySnapshot, Withheld) {
    let geometry = Geometry::of(&snapshot.nodes, window);
    let why: BTreeMap<NodeId, UnseenWhy> = snapshot
        .nodes
        .iter()
        .skip(1)
        .filter_map(|n| why_unseen(n, geometry).map(|w| (n.id, w)))
        .collect();
    if why.is_empty() {
        return (snapshot, Withheld::default());
    }
    let unseen: BTreeSet<NodeId> = why.keys().copied().collect();
    let parents: BTreeMap<NodeId, Parent> =
        snapshot.nodes.iter().map(|n| (n.id, n.parent)).collect();
    let limit = snapshot.nodes.len();
    let (gone, kept): (Vec<A11yNode>, Vec<A11yNode>) = snapshot
        .nodes
        .iter()
        .cloned()
        .partition(|n| unseen.contains(&n.id));
    let nodes: Vec<A11yNode> = kept
        .into_iter()
        .map(|n| A11yNode {
            parent: nearest_seen(n.parent, &unseen, &parents, limit),
            ..n
        })
        .collect();
    let before = match snapshot.truncated {
        Truncation::Complete => 0,
        Truncation::Pruned { dropped } => dropped.0,
    };
    let now = u32::try_from(gone.len()).unwrap_or(u32::MAX);
    let withheld = gone
        .into_iter()
        .filter_map(|n| why.get(&n.id).map(|w| (*w, n)))
        .collect();
    let pruned = A11ySnapshot {
        nodes,
        truncated: Truncation::Pruned {
            dropped: Count(before.saturating_add(now)),
        },
        ..snapshot
    };
    (pruned, Withheld(withheld))
}
