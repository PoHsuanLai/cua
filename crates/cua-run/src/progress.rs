//! Whether the last action changed what the window shows.

use crate::observation::TreeObs;
use a11y_tree::render;
use cua_desktop::Digest32;
use maskcap::FrameDigest;
use serde::{Deserialize, Serialize};

/// What the last action did to the window, from two looks at it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    /// The frame or the tree is different. Also the answer when there is no earlier look.
    #[default]
    Changed,
    /// The frame and the tree (when there is one) are the same as before the action.
    NoVisibleChange,
}

/// What one look at the window held, as digests: the frame, and the rendered tree when there is
/// one. No pixels and no text are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Look {
    frame: FrameDigest,
    tree: Option<Digest32>,
}

impl Look {
    /// The look with this frame digest and this tree.
    pub fn of(frame: FrameDigest, tree: &TreeObs) -> Look {
        let tree = match tree {
            TreeObs::Present(snapshot) => Some(Digest32(
                *blake3::hash(render(snapshot).as_bytes()).as_bytes(),
            )),
            TreeObs::Absent(_) => None,
        };
        Look { frame, tree }
    }

    /// What happened between `before` and this look.
    pub fn effect_since(&self, before: &Look) -> Effect {
        if self == before {
            Effect::NoVisibleChange
        } else {
            Effect::Changed
        }
    }
}
