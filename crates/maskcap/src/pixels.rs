//! A buffer of pixels, and its serialisation for traces.

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use cua_action::{DeviceSize, PixelFormat};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::sync::Arc;

/// Bytes per pixel of every format cua-action names (all four are 32-bit).
pub(crate) const BYTES_PER_PIXEL: usize = 4;

/// A frame counter within one capture session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameSeq(pub u64);

/// Device pixels of one window, row by row.
///
/// Stock cosmic-comp offers only the 8-bit `ABGR8888` and `XBGR8888` shm formats that cua-action
/// names (and 10-bit ones this vocabulary does not carry): the capture backend asks for an 8-bit
/// one. A buffer is screen content: `Debug` prints its shape, never its bytes, and the serde form
/// (base64) is for tests and replay traces.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PixelBuf {
    /// How a pixel is laid out.
    pub format: PixelFormat,
    /// Width and height in device pixels.
    pub size: DeviceSize,
    /// Bytes per row.
    pub stride: u32,
    /// The bytes, `stride * size.h` of them.
    #[serde(with = "base64_bytes")]
    pub data: Arc<[u8]>,
}

impl fmt::Debug for PixelBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PixelBuf({:?} {}x{} stride {}, <{} bytes>)",
            self.format,
            self.size.w,
            self.size.h,
            self.stride,
            self.data.len()
        )
    }
}

mod base64_bytes {
    use super::*;

    pub fn serialize<S: Serializer>(data: &Arc<[u8]>, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&STANDARD.encode(data))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Arc<[u8]>, D::Error> {
        let text = String::deserialize(d)?;
        STANDARD
            .decode(text)
            .map(Arc::from)
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> PixelBuf {
        PixelBuf {
            format: PixelFormat::Xbgr8888,
            size: DeviceSize { w: 2, h: 1 },
            stride: 8,
            data: Arc::from(vec![1u8, 2, 3, 255, 4, 5, 6, 255]),
        }
    }

    #[test]
    fn pixel_buf_round_trips_as_base64_and_pins_its_json() {
        let json = serde_json::to_string(&tiny()).unwrap();
        assert_eq!(
            json,
            r#"{"format":"xbgr8888","size":{"w":2,"h":1},"stride":8,"data":"AQID/wQFBv8="}"#
        );
        assert_eq!(serde_json::from_str::<PixelBuf>(&json).unwrap(), tiny());
    }

    #[test]
    fn debug_never_prints_the_bytes() {
        let shown = format!("{:?}", tiny());
        assert_eq!(shown, "PixelBuf(Xbgr8888 2x1 stride 8, <8 bytes>)");
    }

    #[test]
    fn bad_base64_is_an_error_not_a_panic() {
        let json = r#"{"format":"xbgr8888","size":{"w":1,"h":1},"stride":4,"data":"!!"}"#;
        assert!(serde_json::from_str::<PixelBuf>(json).is_err());
    }
}
