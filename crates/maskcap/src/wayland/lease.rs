//! The lease behind a masked capture: asked of `quire_agent_manager_v1`, answered by `granted`
//! or `revoked`, and the source it gives for the standard copy-capture manager.

use super::state::CapState;
use crate::{CaptureError, MaskRegion};
use casement_protocols::client::quire_agent_v1::quire_agent_lease_v1::{
    self, QuireAgentLeaseV1, RevokeReason,
};
use casement_protocols::client::quire_agent_v1::quire_agent_manager_v1::LeaseMode;
use cua_desktop::ObjectKey as Key;
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum};
use wayland_protocols::ext::image_copy_capture::v1::client::ext_image_copy_capture_manager_v1::Options;

/// Where a lease stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    /// Asked for; no answer yet.
    Asked,
    /// Live: capture may be asked of it.
    Granted,
    /// Ended. The compositor sends nothing after this.
    Revoked(Ended),
}

/// Why a lease ended, as capture sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Ended {
    /// The window was closed.
    WindowClosed,
    /// The compositor refused or withdrew it, or the person's kill chord did.
    Withdrawn,
}

impl Ended {
    /// The error a capture opened on this lease answers with.
    pub(crate) fn error(self) -> CaptureError {
        match self {
            Ended::WindowClosed => CaptureError::SourceGone,
            Ended::Withdrawn => CaptureError::Refused,
        }
    }
}

/// One lease's record.
#[derive(Debug)]
pub(crate) struct LeaseRec {
    proxy: QuireAgentLeaseV1,
    pub standing: Standing,
}

/// The `set_mask_hints` array: five little-endian 32-bit words per region (x, y, width,
/// height, reason).
fn hint_bytes(hints: &[MaskRegion]) -> Vec<u8> {
    hints
        .iter()
        .flat_map(|h| {
            [
                h.rect.origin.x.0,
                h.rect.origin.y.0,
                h.rect.size.w.0,
                h.rect.size.h.0,
                h.why.code(),
            ]
        })
        .flat_map(u32::to_le_bytes)
        .collect()
}

impl CapState {
    /// Asks for a lease on the toplevel named `ident`, in place.
    pub(crate) fn open_lease(&mut self, ident: &str) -> Result<Key, CaptureError> {
        let Some(agent) = &self.agent else {
            return Err(CaptureError::BackendUnavailable);
        };
        let top = self
            .toplevels
            .values()
            .find(|top| top.ident.as_deref() == Some(ident))
            .ok_or(CaptureError::SourceGone)?;
        let proxy = agent.create_lease(&top.handle, LeaseMode::InPlace, &self.qh, ());
        let key = Key::of(&proxy);
        self.leases.insert(
            key,
            LeaseRec {
                proxy,
                standing: Standing::Asked,
            },
        );
        Ok(key)
    }

    /// Where the lease stands; `Revoked` for one that is not there.
    pub(crate) fn standing(&self, lease: Key) -> Standing {
        self.leases
            .get(&lease)
            .map_or(Standing::Revoked(Ended::Withdrawn), |l| l.standing)
    }

    /// Opens a capture session on the lease's masked source.
    pub(crate) fn open_masked_session(&mut self, lease: Key) -> Result<Key, CaptureError> {
        let (Some(rec), Some(copy)) = (self.leases.get(&lease), &self.copy) else {
            return Err(CaptureError::Protocol(
                "the compositor offers no copy capture (ext-image-copy-capture)".into(),
            ));
        };
        let source = rec.proxy.get_capture_source(&self.qh, ());
        let session = copy.create_session(&source, Options::empty(), &self.qh, ());
        source.destroy();
        Ok(self.track(session))
    }

    /// Replaces the regions the compositor is asked to mask.
    pub(crate) fn set_hints(&mut self, lease: Key, hints: &[MaskRegion]) {
        if let Some(rec) = self.leases.get(&lease)
            && rec.standing == Standing::Granted
        {
            rec.proxy.set_mask_hints(hint_bytes(hints));
        }
    }

    /// Ends a lease, if the compositor has not already.
    pub(crate) fn close_lease(&mut self, lease: Key) {
        if let Some(rec) = self.leases.remove(&lease)
            && !matches!(rec.standing, Standing::Revoked(_))
        {
            rec.proxy.destroy();
        }
    }
}

impl Dispatch<QuireAgentLeaseV1, ()> for CapState {
    fn event(
        state: &mut Self,
        lease: &QuireAgentLeaseV1,
        event: quire_agent_lease_v1::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        use quire_agent_lease_v1::Event;
        let Some(rec) = state.leases.get_mut(&Key::of(lease)) else {
            return;
        };
        match event {
            Event::Granted => rec.standing = Standing::Granted,
            Event::Revoked { reason } => {
                rec.standing = Standing::Revoked(match reason {
                    WEnum::Value(RevokeReason::WindowClosed) => Ended::WindowClosed,
                    _ => Ended::Withdrawn,
                });
                // The last event: the client destroys the object.
                lease.destroy();
            }
            _ => {}
        }
    }
}
