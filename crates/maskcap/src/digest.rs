//! A digest of a frame: stall detection and the event log use it, and no pixels are stored.

use crate::pixels::{BYTES_PER_PIXEL, PixelBuf};
use cua_desktop::Digest32;
use serde::{Deserialize, Serialize};

/// The digest of one masked frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameDigest(pub Digest32);

/// The digest of `buf`: equal buffers (same format, size and bytes, stride padding ignored)
/// give equal digests and one changed pixel gives another. A buffer whose data is shorter than
/// its size says is hashed as far as the data goes, with the shortfall marked, so it never
/// collides with a full one. Pure.
pub fn digest(buf: &PixelBuf) -> FrameDigest {
    let mut hasher = blake3::Hasher::new();
    hasher.update(format!("{:?}", buf.format).as_bytes());
    hasher.update(&buf.size.w.to_le_bytes());
    hasher.update(&buf.size.h.to_le_bytes());
    let row_bytes = usize::try_from(buf.size.w)
        .unwrap_or(usize::MAX)
        .saturating_mul(BYTES_PER_PIXEL);
    let stride = usize::try_from(buf.stride).unwrap_or(usize::MAX);
    for row in 0..usize::try_from(buf.size.h).unwrap_or(usize::MAX) {
        let start = row.saturating_mul(stride);
        let end = start.saturating_add(row_bytes);
        match buf.data.get(start..end) {
            Some(bytes) => hasher.update(bytes),
            None => {
                hasher.update(b"short");
                break;
            }
        };
    }
    FrameDigest(Digest32(*hasher.finalize().as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{DeviceSize, PixelFormat};
    use std::sync::Arc;

    fn buf(format: PixelFormat, w: u32, h: u32, fill: u8) -> PixelBuf {
        PixelBuf {
            format,
            size: DeviceSize { w, h },
            stride: w * 4,
            data: Arc::from(vec![fill; (w * h * 4) as usize]),
        }
    }

    #[test]
    fn the_format_and_the_size_are_part_of_the_digest() {
        let base = digest(&buf(PixelFormat::Xbgr8888, 4, 4, 9));
        assert_ne!(base, digest(&buf(PixelFormat::Xrgb8888, 4, 4, 9)), "format");
        assert_ne!(
            digest(&buf(PixelFormat::Xbgr8888, 8, 2, 9)),
            digest(&buf(PixelFormat::Xbgr8888, 2, 8, 9)),
            "the same bytes in another shape"
        );
        assert_eq!(base, digest(&buf(PixelFormat::Xbgr8888, 4, 4, 9)));
    }

    #[test]
    fn a_short_buffer_is_hashed_without_panicking_and_differs_from_a_full_one() {
        let full = buf(PixelFormat::Abgr8888, 4, 4, 1);
        let short = PixelBuf {
            data: Arc::from(vec![1u8; 20]),
            ..full.clone()
        };
        assert_ne!(digest(&full), digest(&short));
        let empty = PixelBuf {
            data: Arc::from(Vec::<u8>::new()),
            ..full.clone()
        };
        assert_ne!(digest(&short), digest(&empty));
    }
}
