//! The capture seams.

use crate::frame::Frame;
use crate::mask::MaskRegion;
use crate::settle::DamageAt;
use cua_desktop::LeaseGrant;
use serde::{Deserialize, Serialize};
use std::future::Future;

/// When a frame is wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FrameWant {
    /// The current content. A compositor that sends frames only on damage may hold the frame of
    /// a still window; after the settle window (`AFTER_DAMAGE_LIMIT_MS`) the last frame is the
    /// current one, with no damage.
    Now,
    /// The first frame after the content changed (on stock: after a pixel diff). A session waits
    /// at most `AFTER_DAMAGE_LIMIT`; a window that stayed still answers with its current frame
    /// and no damage.
    AfterDamage,
}

/// The longest a session waits for a changed frame under either want once it has a frame to
/// fall back on, in milliseconds.
pub const AFTER_DAMAGE_LIMIT_MS: u64 = 1000;

/// Why a capture failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CaptureError {
    /// The window is gone.
    #[error("the window is gone")]
    SourceGone,
    /// The compositor refused the source.
    #[error("the compositor refused the capture")]
    Refused,
    /// The compositor withholds frames of a hidden window.
    #[error("hidden windows are not rendered")]
    Withheld,
    /// No pixel format this crate can read.
    #[error("no usable pixel format")]
    Format,
    /// The compositor's answer did not follow the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
    /// The capture path is declared but not built yet.
    #[error("this capture path is not built")]
    BackendUnavailable,
}

/// Opens capture sessions on a leased window.
pub trait WindowCapture: Send + Sync {
    /// The session type this source opens.
    type Session: CaptureSession;

    /// Opens a session on the lease's window.
    fn open(
        &self,
        lease: &LeaseGrant,
    ) -> impl Future<Output = Result<Self::Session, CaptureError>> + Send;
}

/// One open capture of one window.
pub trait CaptureSession: Send {
    /// A masked frame. `hints` are the regions cuad wants masked; on the fork they go to the
    /// compositor, on stock `apply_masks` runs here (`MaskReport::applied_by` says which).
    fn frame(
        &mut self,
        want: FrameWant,
        hints: &[MaskRegion],
    ) -> impl Future<Output = Result<Frame, CaptureError>> + Send;

    /// The next change in the window.
    fn damage(&mut self) -> impl Future<Output = Result<DamageAt, CaptureError>> + Send;
}
