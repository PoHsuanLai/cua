//! The backends cuad chooses between: closed sets, so enums that implement the seams.

use cua_desktop::{BackendCaps, BackendKind, LeaseGrant, MaskWhere};
use cua_desktop::{EiSocket, LeaseId};
use maskcap::{
    CaptureError, CaptureSession, DamageAt, Frame, FrameWant, MaskRegion, QuireMaskedCapture,
    StampClock, StockToplevelCapture, WaylandSession, WindowCapture,
};
use seat_input::{
    AgentSeat, EiSeat, Emulation, InputOp, InputReceipt, Keymap, SeatCaps, SeatError,
};
use std::sync::Arc;
use wayland_client::Connection;

/// Which capture backend a desktop backend uses: the stock toplevel source with masking in
/// cuad, or the fork's masked source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CaptureKind {
    /// `ext_foreign_toplevel_image_capture_source_manager_v1`, masked by `maskcap::apply_masks`.
    StockToplevel,
    /// The lease's `get_capture_source`, masked by the compositor.
    QuireMasked,
}

/// The capture backend for a desktop backend: the fork's masked source where the compositor
/// masks, the stock source (and `apply_masks` in cuad) everywhere else.
pub fn capture_kind(kind: BackendKind) -> CaptureKind {
    match BackendCaps::of(kind).masking {
        MaskWhere::Compositor => CaptureKind::QuireMasked,
        MaskWhere::Cuad => CaptureKind::StockToplevel,
    }
}

/// Capture over either backend.
#[derive(Debug)]
pub enum CaptureBackend {
    /// Stock toplevel capture.
    StockToplevel(StockToplevelCapture),
    /// The fork's masked source.
    QuireMasked(QuireMaskedCapture),
    /// No compositor to capture from: every `open` is refused.
    Unavailable,
}

impl CaptureBackend {
    /// The backend of `kind` over an open connection.
    pub fn open_kind(kind: CaptureKind, connection: Connection) -> Self {
        match kind {
            CaptureKind::StockToplevel => {
                CaptureBackend::StockToplevel(StockToplevelCapture::over(connection))
            }
            CaptureKind::QuireMasked => {
                CaptureBackend::QuireMasked(QuireMaskedCapture::over(connection))
            }
        }
    }
}

impl CaptureBackend {
    /// The same backend, stamping its frames with `clock`: the stock source takes it, the
    /// others have no frames of their own to stamp.
    #[must_use]
    pub fn with_clock(self, clock: Arc<dyn StampClock>) -> Self {
        match self {
            CaptureBackend::StockToplevel(capture) => {
                CaptureBackend::StockToplevel(capture.with_clock(clock))
            }
            other => other,
        }
    }
}

impl WindowCapture for CaptureBackend {
    type Session = CaptureSessionKind;

    async fn open(&self, lease: &LeaseGrant) -> Result<CaptureSessionKind, CaptureError> {
        match self {
            CaptureBackend::StockToplevel(capture) => {
                capture.open(lease).await.map(CaptureSessionKind::Stock)
            }
            CaptureBackend::QuireMasked(capture) => {
                capture.open(lease).await.map(CaptureSessionKind::Masked)
            }
            CaptureBackend::Unavailable => Err(CaptureError::Refused),
        }
    }
}

/// An open capture session of either backend.
#[derive(Debug)]
pub enum CaptureSessionKind {
    /// Over the stock source.
    Stock(WaylandSession),
    /// Over the masked source.
    Masked(WaylandSession),
}

impl CaptureSession for CaptureSessionKind {
    async fn frame(
        &mut self,
        want: FrameWant,
        hints: &[MaskRegion],
    ) -> Result<Frame, CaptureError> {
        match self {
            CaptureSessionKind::Stock(session) | CaptureSessionKind::Masked(session) => {
                session.frame(want, hints).await
            }
        }
    }

    async fn damage(&mut self) -> Result<DamageAt, CaptureError> {
        match self {
            CaptureSessionKind::Stock(session) | CaptureSessionKind::Masked(session) => {
                session.damage().await
            }
        }
    }
}

/// The agent's seat: EI through reis, to the fork's lease socket or stock's `GetSenderSocket`.
#[derive(Debug)]
pub enum SeatBackend {
    /// An EIS connection.
    Ei(EiSeat),
}

impl AgentSeat for SeatBackend {
    fn caps(&self) -> SeatCaps {
        match self {
            SeatBackend::Ei(seat) => seat.caps(),
        }
    }

    fn keymap(&self) -> &Keymap {
        match self {
            SeatBackend::Ei(seat) => seat.keymap(),
        }
    }

    async fn attach(&self, lease: &LeaseGrant, socket: EiSocket) -> Result<(), SeatError> {
        match self {
            SeatBackend::Ei(seat) => seat.attach(lease, socket).await,
        }
    }

    async fn perform(&self, lease: LeaseId, ops: &[InputOp]) -> Result<InputReceipt, SeatError> {
        match self {
            SeatBackend::Ei(seat) => seat.perform(lease, ops).await,
        }
    }

    async fn emulation(&self, lease: LeaseId, state: Emulation) -> Result<(), SeatError> {
        match self {
            SeatBackend::Ei(seat) => seat.emulation(lease, state).await,
        }
    }

    async fn detach(&self, lease: LeaseId) -> Result<(), SeatError> {
        match self {
            SeatBackend::Ei(seat) => seat.detach(lease).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_follows_who_masks() {
        let table = [
            (BackendKind::CosmicStock, CaptureKind::StockToplevel),
            (BackendKind::KwinStock, CaptureKind::StockToplevel),
        ];
        for (kind, want) in table {
            assert_eq!(capture_kind(kind), want, "{kind:?}");
        }
    }
}
