//! Finding nodes again by role and name, for procedures (a later wave). The types are frozen so
//! `SuccessCheck` can name them. The selector itself lives in `cua-types`, re-exported here.

use cua_action::NodeId;
pub use cua_types::{NameMatch, NodeSelector, SelectorStep, SlotName};
use serde::{Deserialize, Serialize};

/// What a selector found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Resolution {
    /// Exactly one.
    One(NodeId),
    /// More than one.
    Many(Vec<NodeId>),
    /// None.
    None,
}
