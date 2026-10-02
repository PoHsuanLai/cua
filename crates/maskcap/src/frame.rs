//! One captured frame, already masked.

use crate::mask::MaskReport;
use crate::pixels::{FrameSeq, PixelBuf};
use cua_action::{Rect, Scale120, Size, WindowSpace};
use cua_desktop::Stamp;
use serde::{Deserialize, Serialize};

/// A masked frame of the leased window, cropped to its geometry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Frame {
    /// Which frame of the session.
    pub seq: FrameSeq,
    /// When it arrived.
    pub at: Stamp,
    /// The masked pixels.
    pub pixels: PixelBuf,
    /// The window's logical size.
    pub logical: Size<WindowSpace>,
    /// Device pixels per logical pixel, in 120ths: 120 means the buffer is at logical
    /// resolution. Stock cosmic-comp captures at logical resolution at every output scale
    /// (measured at 1.5), so it reports 120 there.
    pub scale: Scale120,
    /// What changed since the previous frame of this session. On stock this is the whole window
    /// every time; use the pixel diff.
    pub damage: Vec<Rect<WindowSpace>>,
    /// What was masked and by whom.
    pub masks: MaskReport,
}
