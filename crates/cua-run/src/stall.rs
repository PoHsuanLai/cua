//! Whether the screen has stopped changing.

use maskcap::FrameDigest;
use porter_ids::Count;
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

/// The newest frame digest and how many frames in a row have had it. This is all stall
/// detection needs, so a long run keeps a fixed amount of history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repeats {
    last: Option<FrameDigest>,
    count: Count,
}

impl Default for Repeats {
    fn default() -> Self {
        Self {
            last: None,
            count: Count(0),
        }
    }
}

impl Repeats {
    /// The history after one more frame: the count grows when the digest equals the last one and
    /// restarts at one when it does not.
    #[must_use]
    pub fn seen(self, digest: FrameDigest) -> Self {
        let count = match self.last {
            Some(last) if last == digest => Count(self.count.0.saturating_add(1)),
            _ => Count(1),
        };
        Self {
            last: Some(digest),
            count,
        }
    }

    /// Equal frames in a row, newest first (zero before the first frame).
    pub fn count(&self) -> Count {
        self.count
    }

    /// `Stalled` when the newest `limit` frames are all equal; `Moving` otherwise, and always
    /// `Moving` for a limit of zero. Pure.
    pub fn stall(&self, limit: Count) -> Stall {
        if limit.0 == 0 || self.count.0 < limit.0 {
            Stall::Moving
        } else {
            Stall::Stalled
        }
    }
}
