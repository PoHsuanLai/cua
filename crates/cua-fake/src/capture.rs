//! Capture that returns the frames it was given.

use cua_desktop::LeaseGrant;
use maskcap::{
    CaptureError, CaptureSession, DamageAt, Frame, FrameWant, MaskRegion, WindowCapture,
};
use std::collections::VecDeque;
use std::sync::Mutex;

/// A [`WindowCapture`] that opens sessions over scripted frames and damage. Each session gets a
/// copy of the script; when it runs out the window "is gone".
#[derive(Debug, Default)]
pub struct FakeCapture {
    frames: Vec<Frame>,
    damage: Vec<DamageAt>,
    refuse: Mutex<Option<CaptureError>>,
}

impl FakeCapture {
    /// A capture over these frames and damage events.
    pub fn scripted(frames: Vec<Frame>, damage: Vec<DamageAt>) -> Self {
        Self {
            frames,
            damage,
            refuse: Mutex::new(None),
        }
    }

    /// Makes the next `open` fail with `error`.
    pub fn refuse_next(&self, error: CaptureError) {
        *self.refuse.lock().expect("fake capture lock") = Some(error);
    }
}

/// One open session of a [`FakeCapture`].
#[derive(Debug)]
pub struct FakeCaptureSession {
    frames: VecDeque<Frame>,
    damage: VecDeque<DamageAt>,
    hints: Vec<Vec<MaskRegion>>,
}

impl FakeCaptureSession {
    /// The mask hints each `frame` call was given, in order.
    pub fn hints_seen(&self) -> &[Vec<MaskRegion>] {
        &self.hints
    }
}

impl WindowCapture for FakeCapture {
    type Session = FakeCaptureSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<FakeCaptureSession, CaptureError> {
        let _ = lease;
        if let Some(error) = self.refuse.lock().expect("fake capture lock").take() {
            return Err(error);
        }
        Ok(FakeCaptureSession {
            frames: self.frames.iter().cloned().collect(),
            damage: self.damage.iter().copied().collect(),
            hints: Vec::new(),
        })
    }
}

impl CaptureSession for FakeCaptureSession {
    async fn frame(
        &mut self,
        want: FrameWant,
        hints: &[MaskRegion],
    ) -> Result<Frame, CaptureError> {
        let _ = want;
        self.hints.push(hints.to_vec());
        self.frames.pop_front().ok_or(CaptureError::SourceGone)
    }

    async fn damage(&mut self) -> Result<DamageAt, CaptureError> {
        self.damage.pop_front().ok_or(CaptureError::SourceGone)
    }
}
