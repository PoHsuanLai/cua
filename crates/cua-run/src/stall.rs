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
    let Ok(limit) = usize::try_from(limit.0) else {
        return Stall::Moving;
    };
    if limit == 0 || history.len() < limit {
        return Stall::Moving;
    }
    let newest = &history[history.len() - limit..];
    match newest.split_first() {
        Some((first, rest)) if rest.iter().all(|d| d == first) => Stall::Stalled,
        _ => Stall::Moving,
    }
}
