//! ext-image-copy-capture over a wayland connection: the stock toplevel source (masking in
//! cuad) and the quire masked source (masking in the compositor, over an agent lease).
//!
//! Stock frames are unmasked and not damage-gated (Spike A): `frame` applies the caller's mask
//! hints with [`apply_masks`] and `damage` is a pixel diff between consecutive frames. Capture
//! asks for an 8-bit shm format and reads the buffer back through its memfd, so nothing here
//! maps memory.

mod key;
mod lease;
mod pump;
mod session;
mod shm;
mod state;

pub use key::{ObjectKey, Rounds, SyncTag};
pub use pump::{Pump, PumpError};
pub use session::{StampClock, WaylandSession};

use crate::key::ObjectKey as Key;
use cua_desktop::LeaseGrant;
use lease::Standing;
use maskcap::{CaptureError, WindowCapture};
use session::{Masking, SinceStart};
use state::CapState;
use std::sync::Arc;
use wayland_client::Connection;

/// Round trips that bring a fresh connection to rest: the registry, then what was bound from it.
const SETTLE_ROUNDS: u32 = 2;

/// Capture through `ext_foreign_toplevel_image_capture_source_manager_v1`: frames arrive
/// unmasked and the caller masks them. Asks for an 8-bit shm format (`ABGR8888`/`XBGR8888`).
#[derive(Debug)]
pub struct StockToplevelCapture {
    pump: Arc<Pump<CapState>>,
    clock: Arc<dyn StampClock>,
}

/// Capture through the lease's `get_capture_source`: the compositor masks before the buffer
/// leaves, so frames are reported as masked by the compositor and the hints go to it. Needs
/// the agent connection (`Compositor1.Connect("agent")`); a compositor that does not advertise
/// `quire_agent_manager_v1` (casement master does not yet) makes `open` answer
/// [`CaptureError::BackendUnavailable`].
#[derive(Debug)]
pub struct QuireMaskedCapture {
    pump: Arc<Pump<CapState>>,
    clock: Arc<dyn StampClock>,
}

impl StockToplevelCapture {
    /// Captures over an open connection. Nothing is asked of the compositor until `open`.
    pub fn over(connection: Connection) -> Self {
        let queue = connection.new_event_queue::<CapState>();
        let state = CapState::new(&connection, queue.handle());
        Self {
            pump: Arc::new(Pump::new(connection, queue, state)),
            clock: Arc::new(SinceStart::now()),
        }
    }

    /// Stamps frames and damage with `clock` instead of milliseconds since construction. cuad
    /// passes its own clock: the run machine compares these stamps with its own.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn StampClock>) -> Self {
        self.clock = clock;
        self
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        self.pump.connection()
    }
}

impl QuireMaskedCapture {
    /// Captures over an open agent connection. Nothing is asked of the compositor until `open`.
    pub fn over(connection: Connection) -> Self {
        let queue = connection.new_event_queue::<CapState>();
        let state = CapState::new(&connection, queue.handle());
        Self {
            pump: Arc::new(Pump::new(connection, queue, state)),
            clock: Arc::new(SinceStart::now()),
        }
    }

    /// Stamps frames and damage with `clock` instead of milliseconds since construction.
    #[must_use]
    pub fn with_clock(mut self, clock: Arc<dyn StampClock>) -> Self {
        self.clock = clock;
        self
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        self.pump.connection()
    }
}

fn lost(_: crate::pump::PumpError) -> CaptureError {
    CaptureError::SourceGone
}

/// Brings a fresh connection to rest: the registry is read and what it offers is bound.
async fn settle(pump: &Pump<CapState>) -> Result<(), CaptureError> {
    for _ in 0..SETTLE_ROUNDS {
        let mut asked = None;
        pump.drive(|st| {
            let n = *asked.get_or_insert_with(|| st.sync());
            st.synced(n).then_some(())
        })
        .await
        .map_err(lost)?;
    }
    Ok(())
}

impl WindowCapture for StockToplevelCapture {
    type Session = WaylandSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<WaylandSession, CaptureError> {
        settle(&self.pump).await?;
        let ident = lease.window.ident.as_str();
        let key = self
            .pump
            .drive(|st| Some(st.open_session(ident)))
            .await
            .map_err(lost)??;
        WaylandSession::start(self.pump.clone(), self.clock.clone(), key, Masking::Cuad).await
    }
}

impl WindowCapture for QuireMaskedCapture {
    type Session = WaylandSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<WaylandSession, CaptureError> {
        settle(&self.pump).await?;
        let ident = lease.window.ident.as_str();
        let held = self
            .pump
            .drive(|st| Some(st.open_lease(ident)))
            .await
            .map_err(lost)??;
        let answered = self
            .pump
            .drive(|st| match st.standing(held) {
                Standing::Asked => None,
                other => Some(other),
            })
            .await
            .map_err(lost);
        let opened = match answered {
            Ok(Standing::Revoked(ended)) => Err(ended.error()),
            Ok(_) => self.start(held).await,
            Err(e) => Err(e),
        };
        if opened.is_err() {
            self.pump.try_with(|st| st.close_lease(held));
        }
        opened
    }
}

impl QuireMaskedCapture {
    /// The session on a granted lease. The session owns the lease from here.
    async fn start(&self, held: Key) -> Result<WaylandSession, CaptureError> {
        let key = self
            .pump
            .drive(|st| Some(st.open_masked_session(held)))
            .await
            .map_err(lost)??;
        WaylandSession::start(
            self.pump.clone(),
            self.clock.clone(),
            key,
            Masking::Compositor(held),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use casement_protocols::client::quire_agent_v1::quire_agent_lease_v1::MaskReason as ProtocolReason;
    use maskcap::MaskReason;

    #[test]
    fn mask_reason_codes_equal_the_protocol_enum() {
        let table = [
            (
                MaskReason::PasswordField,
                ProtocolReason::PasswordField as u32,
            ),
            (
                MaskReason::SurfaceDeclared,
                ProtocolReason::SurfaceDeclared as u32,
            ),
            (MaskReason::A11ySecret, ProtocolReason::A11ySecret as u32),
            (
                MaskReason::SensitiveWindow,
                ProtocolReason::SensitiveWindow as u32,
            ),
            (MaskReason::OtherSpace, ProtocolReason::OtherSpace as u32),
            (
                MaskReason::OverlayOutsideLease,
                ProtocolReason::OverlayOutsideLease as u32,
            ),
        ];
        for (reason, want) in table {
            assert_eq!(reason.code(), want, "{reason:?}");
        }
    }
}
