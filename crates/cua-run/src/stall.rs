//! Whether the screen has stopped changing.

use maskcap::FrameDigest;
use porter_core::Count;
use serde::{Deserialize, Serialize};

/// Whether the frames are still changing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stall {
    /// They are.
    Moving,
    /// The last `limit` digests are equal.
    Stalled,
}

/// `Stalled` when the newest `limit` digests are all equal; `Moving` otherwise, and always
/// `Moving` for a limit of zero or fewer digests than the limit. Pure.
pub fn stalled(history: &[FrameDigest], limit: Count) -> Stall {
    let _ = (history, limit);
    todo!("stall_after_n_equal_digests")
}
