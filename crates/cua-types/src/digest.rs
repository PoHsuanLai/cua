//! The digest of a masked frame, as the event log records it.

use cua_desktop::Digest32;
use serde::{Deserialize, Serialize};

/// The digest of one masked frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameDigest(pub Digest32);
