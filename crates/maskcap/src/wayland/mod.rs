//! ext-image-copy-capture over a wayland connection: the stock toplevel source (masking in
//! cuad) and the quire masked source (masking in the compositor, not built yet).
//!
//! Stock frames are unmasked and not damage-gated (Spike A): `frame` applies the caller's mask
//! hints with [`apply_masks`] and `damage` is a pixel diff between consecutive frames. Capture
//! asks for an 8-bit shm format and reads the buffer back through its memfd, so nothing here
//! maps memory.

mod session;
mod shm;
mod state;

pub use session::{StampClock, WaylandSession};

use crate::{CaptureError, WindowCapture};
use cua_desktop::{LeaseGrant, Pump};
use session::SinceStart;
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
/// leaves. Not built: it needs casement's agent socket (P2-Q2, `Compositor1.Connect("agent")`),
/// which does not exist yet; `open` answers [`CaptureError::BackendUnavailable`] until then.
#[derive(Debug)]
pub struct QuireMaskedCapture {
    connection: Connection,
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
    /// Captures over an open connection.
    pub fn over(connection: Connection) -> Self {
        Self { connection }
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

fn lost(_: cua_desktop::PumpError) -> CaptureError {
    CaptureError::SourceGone
}

impl WindowCapture for StockToplevelCapture {
    type Session = WaylandSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<WaylandSession, CaptureError> {
        for _ in 0..SETTLE_ROUNDS {
            let mut asked = None;
            self.pump
                .drive(|st| {
                    let n = *asked.get_or_insert_with(|| st.sync());
                    st.synced(n).then_some(())
                })
                .await
                .map_err(lost)?;
        }
        let ident = lease.window.ident.as_str();
        let key = self
            .pump
            .drive(|st| Some(st.open_session(ident)))
            .await
            .map_err(lost)??;
        WaylandSession::start(self.pump.clone(), self.clock.clone(), key).await
    }
}

impl WindowCapture for QuireMaskedCapture {
    type Session = WaylandSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<WaylandSession, CaptureError> {
        // Not built: needs casement's agent socket (P2-Q2): lease.get_capture_source,
        // create_session over it, set_mask_hints before each frame.
        let _ = (lease, &self.connection);
        Err(CaptureError::BackendUnavailable)
    }
}

#[cfg(test)]
mod tests {
    use crate::MaskReason;
    use casement_protocols::client::quire_agent_v1::quire_agent_lease_v1::MaskReason as ProtocolReason;

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
