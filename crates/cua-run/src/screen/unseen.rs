//! Text the tree carries and the person cannot see.
//!
//! A node can hold words that are not on the screen: it has no size, it stands to the right of
//! the window, or the tree says it is neither visible nor showing. The model reads the whole
//! tree, so such a node is a place to hide a sentence for it. Short labels are not worth a
//! question (a zero-size separator named "Close"), so only a node with a sentence's worth of
//! words counts ([`MIN_WORDS`]).
//!
//! What is not judged, on purpose: a node below the bottom edge (a long page scrolled to its
//! top is full of them), a node whose bounds are unknown (a tree where no node has any extent has
//! no geometry to judge), and a tree that sets no state bits at all.

use a11y_tree::{A11yNode, NodeState};
use cua_action::{Size, WindowSpace};
use serde::{Deserialize, Serialize};

/// How many words a node's text needs before unseen text is worth a question.
pub const MIN_WORDS: usize = 4;

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
