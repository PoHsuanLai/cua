//! Masked per-window capture.
//!
//! A [`Frame`] is a window's pixels with the regions that must not reach a model already blacked
//! ([`MaskReport`] says who did it: the compositor on the fork, cuad on stock). The crate also
//! holds the digest used for stall detection and the event log ([`FrameDigest`]), the settling
//! decision ([`settle`], [`DamageAt`], [`pixel_diff`]) and the two seams ([`WindowCapture`],
//! [`CaptureSession`]).
//!
//! Pure: the backend, ext-image-copy-capture over wayland-client, is `maskcap-wayland`.

mod capture;
mod digest;
mod frame;
mod mask;
mod pixels;
mod settle;

pub use capture::{AFTER_DAMAGE_LIMIT_MS, CaptureError, CaptureSession, FrameWant, WindowCapture};
pub use digest::{FrameDigest, digest};
pub use frame::Frame;
pub use mask::{MaskReason, MaskRegion, MaskReport, WholeMask, apply_masks};
pub use pixels::{FrameSeq, PixelBuf};
pub use settle::{DamageAt, DamageSource, Settle, pixel_diff, settle};
