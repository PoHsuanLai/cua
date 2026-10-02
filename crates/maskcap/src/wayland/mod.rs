//! ext-image-copy-capture over a wayland connection: the stock toplevel source (masking in
//! cuad) and the quire masked source (masking in the compositor).

use crate::{CaptureError, CaptureSession, DamageAt, Frame, FrameWant, MaskRegion, WindowCapture};
use cua_desktop::LeaseGrant;
use wayland_client::Connection;

/// Capture through `ext_foreign_toplevel_image_capture_source_manager_v1`: frames arrive
/// unmasked and the caller masks them. Asks for an 8-bit shm format (`ABGR8888`/`XBGR8888`).
#[derive(Debug)]
pub struct StockToplevelCapture {
    connection: Connection,
}

/// Capture through the lease's `get_capture_source`: the compositor masks before the buffer
/// leaves.
#[derive(Debug)]
pub struct QuireMaskedCapture {
    connection: Connection,
}

/// A session over a source of either kind.
#[derive(Debug)]
pub struct WaylandSession {
    connection: Connection,
}

impl StockToplevelCapture {
    /// Captures over an open connection.
    pub fn over(connection: Connection) -> Self {
        Self { connection }
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
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

impl WindowCapture for StockToplevelCapture {
    type Session = WaylandSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<WaylandSession, CaptureError> {
        let _ = lease;
        todo!(
            "create_source from the toplevel handle, create_session, read buffer_size and shm_format"
        )
    }
}

impl WindowCapture for QuireMaskedCapture {
    type Session = WaylandSession;

    async fn open(&self, lease: &LeaseGrant) -> Result<WaylandSession, CaptureError> {
        let _ = lease;
        todo!("lease.get_capture_source, create_session over it")
    }
}

impl CaptureSession for WaylandSession {
    async fn frame(
        &mut self,
        want: FrameWant,
        hints: &[MaskRegion],
    ) -> Result<Frame, CaptureError> {
        let _ = (want, hints, &self.connection);
        todo!(
            "attach a shm buffer, capture, wait ready; stock: apply_masks; fork: set_mask_hints first"
        )
    }

    async fn damage(&mut self) -> Result<DamageAt, CaptureError> {
        todo!("stock: poll frames and diff pixels; fork: the session's damage events")
    }
}

#[cfg(test)]
mod tests {
    use crate::MaskReason;
    use quire_agent_protocol::client::quire_agent_lease_v1::MaskReason as ProtocolReason;

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
