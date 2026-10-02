//! A digest of a frame: stall detection and the event log use it, and no pixels are stored.

use crate::pixels::PixelBuf;
use cua_desktop::Digest32;
use serde::{Deserialize, Serialize};

/// The digest of one masked frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameDigest(pub Digest32);

/// The digest of `buf`: equal buffers (same format, size and bytes, stride padding ignored)
/// give equal digests and one changed pixel gives another. Pure.
pub fn digest(buf: &PixelBuf) -> FrameDigest {
    let _ = buf;
    todo!("a 256-bit hash of format, size and the unpadded rows (digest_differs_on_one_pixel)")
}
