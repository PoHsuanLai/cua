//! One open capture of one window.

use super::shm::Shared;
use super::state::{CapState, Constraints, Failure, FrameSlot, pick_format};
use crate::{
    AFTER_DAMAGE_LIMIT_MS, CaptureError, CaptureSession, DamageAt, DamageSource, Frame, FrameSeq,
    FrameWant, MaskRegion, MaskReport, PixelBuf, WholeMask, apply_masks, pixel_diff,
};
use cua_action::{Coord, Point, Rect, Scale120, Size, WindowSpace};
use cua_desktop::ObjectKey as Key;
use cua_desktop::{MaskWhere, Pump, Stamp};
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Where frame and damage stamps come from.
pub trait StampClock: Send + Sync + fmt::Debug {
    /// Now, on whatever clock the caller's run machine uses.
    fn now(&self) -> Stamp;
}

/// Milliseconds since the capture was constructed: the default, for callers with no clock of
/// their own.
#[derive(Debug)]
pub(crate) struct SinceStart(Instant);

impl SinceStart {
    pub(crate) fn now() -> Self {
        Self(Instant::now())
    }
}

impl StampClock for SinceStart {
    fn now(&self) -> Stamp {
        Stamp(u64::try_from(self.0.elapsed().as_millis()).unwrap_or(u64::MAX))
    }
}

/// Stock captures at logical resolution at every output scale (Spike A, measured at 1.5).
const LOGICAL: Scale120 = Scale120(120);

/// The pause between two looks at a window that has not changed.
const PACE: Duration = Duration::from_millis(50);

/// A session over a source of either kind.
#[derive(Debug)]
pub struct WaylandSession {
    pump: Arc<Pump<CapState>>,
    clock: Arc<dyn StampClock>,
    key: Key,
    buffer: Shared,
    /// The last unmasked frame, what the next pixel diff compares with.
    last: Option<PixelBuf>,
    seq: u64,
}

fn lost(_: cua_desktop::PumpError) -> CaptureError {
    CaptureError::SourceGone
}

fn whole(buf: &PixelBuf) -> Rect<WindowSpace> {
    Rect {
        origin: Point::new(Coord(0), Coord(0)),
        size: Size::new(Coord(buf.size.w), Coord(buf.size.h)),
    }
}

impl WaylandSession {
    /// Waits for the session's constraints and makes the buffer for them.
    pub(crate) async fn start(
        pump: Arc<Pump<CapState>>,
        clock: Arc<dyn StampClock>,
        key: Key,
    ) -> Result<Self, CaptureError> {
        let buffer = Self::buffer_for(&pump, key, 0).await?;
        Ok(Self {
            pump,
            clock,
            key,
            buffer,
            last: None,
            seq: 0,
        })
    }

    /// A buffer that fits the constraints once they are known and newer than `after`.
    async fn buffer_for(
        pump: &Pump<CapState>,
        key: Key,
        after: u32,
    ) -> Result<Shared, CaptureError> {
        pump.drive(|st| {
            let rec = st.sessions.get(&key)?;
            match rec.constraints {
                Constraints::Pending => None,
                Constraints::Stopped => Some(Err(CaptureError::SourceGone)),
                Constraints::Known if rec.epoch <= after => None,
                Constraints::Known => {
                    let (size, epoch, formats) = (rec.size, rec.epoch, rec.formats.clone());
                    Some(match pick_format(&formats) {
                        None => Err(CaptureError::Format),
                        Some((wire, format)) => Shared::make(st, size, epoch, wire, format),
                    })
                }
            }
        })
        .await
        .map_err(lost)?
    }

    /// One unmasked frame and the damage the compositor reported for it.
    async fn capture(&mut self) -> Result<PixelBuf, CaptureError> {
        loop {
            let (key, size) = (self.key, self.buffer.size);
            let mut asked = false;
            let buffer = self.buffer.buffer.clone();
            let slot = self
                .pump
                .drive(|st| {
                    if !asked {
                        st.request_frame(key, &buffer, size);
                        asked = true;
                    }
                    match st.sessions.get(&key)?.frame.clone() {
                        FrameSlot::Idle | FrameSlot::Waiting => None,
                        done => Some(done),
                    }
                })
                .await
                .map_err(lost)?;
            match slot {
                FrameSlot::Ready(_) => return self.buffer.read(),
                FrameSlot::Failed(Failure::Constraints) => {
                    let old = self.buffer.epoch;
                    let next = Self::buffer_for(&self.pump, self.key, old).await?;
                    std::mem::replace(&mut self.buffer, next).destroy();
                }
                FrameSlot::Failed(Failure::Stopped) => return Err(CaptureError::SourceGone),
                FrameSlot::Failed(Failure::Unknown) => {
                    return Err(CaptureError::Protocol(
                        "the compositor failed the frame".into(),
                    ));
                }
                FrameSlot::Idle | FrameSlot::Waiting => {
                    return Err(CaptureError::Protocol("no frame state".into()));
                }
            }
        }
    }

    /// Frames until one differs from the last, or the first when there was none: what changed
    /// and when.
    async fn next_change(&mut self) -> Result<(PixelBuf, Rect<WindowSpace>), CaptureError> {
        loop {
            let raw = self.capture().await?;
            let change = match &self.last {
                None => Some(whole(&raw)),
                Some(last) => pixel_diff(last, &raw),
            };
            self.last = Some(raw.clone());
            if let Some(region) = change {
                return Ok((raw, region));
            }
            tokio::time::sleep(PACE).await;
        }
    }
}

impl CaptureSession for WaylandSession {
    async fn frame(
        &mut self,
        want: FrameWant,
        hints: &[MaskRegion],
    ) -> Result<Frame, CaptureError> {
        let (raw, damage) = match want {
            FrameWant::AfterDamage => {
                let limit = Duration::from_millis(AFTER_DAMAGE_LIMIT_MS);
                match tokio::time::timeout(limit, self.next_change()).await {
                    Ok(changed) => {
                        let (raw, region) = changed?;
                        (raw, vec![region])
                    }
                    // Nothing moved: the window is still, so the last look is the current frame.
                    Err(_) => match self.last.clone() {
                        Some(raw) => (raw, Vec::new()),
                        None => (self.capture().await?, Vec::new()),
                    },
                }
            }
            FrameWant::Now => {
                let raw = self.capture().await?;
                let region = match &self.last {
                    None => Some(whole(&raw)),
                    Some(last) => pixel_diff(last, &raw),
                };
                self.last = Some(raw.clone());
                (raw, region.into_iter().collect())
            }
        };
        let logical = Size::new(Coord(raw.size.w), Coord(raw.size.h));
        let (pixels, regions) = apply_masks(raw, LOGICAL, hints);
        self.seq += 1;
        Ok(Frame {
            seq: FrameSeq(self.seq),
            at: self.clock.now(),
            pixels,
            logical,
            scale: LOGICAL,
            damage,
            masks: MaskReport {
                applied_by: MaskWhere::Cuad,
                whole: WholeMask::None,
                regions,
            },
        })
    }

    async fn damage(&mut self) -> Result<DamageAt, CaptureError> {
        let (_, region) = self.next_change().await?;
        Ok(DamageAt {
            at: self.clock.now(),
            region,
            source: DamageSource::PixelDiff,
        })
    }
}

impl Drop for WaylandSession {
    fn drop(&mut self) {
        self.buffer.destroy();
        let key = self.key;
        // If another task has the pump mid-turn the session's objects stay until the
        // connection closes: a drop cannot wait.
        self.pump.try_with(|st| st.close_session(key));
    }
}
