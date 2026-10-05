//! The stock lease: one run per window, input on the shared seat through `GetSenderSocket`.

use super::socket::{DeviceTypes, SenderSocket};
use crate::{
    BackendKind, DesktopError, DesktopEvent, EiOrigin, EiSocket, LeaseGrant, LeaseId, LeaseRequest,
    Leased, RevokeReason, RunMode, SeatIsolation, ToplevelIdent, ToplevelInfo,
};
use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

/// The leases this desktop has granted and not yet ended.
#[derive(Debug, Default)]
pub(crate) struct Leases {
    next: u64,
    held: BTreeMap<LeaseId, ToplevelIdent>,
}

impl Leases {
    fn take(&mut self, window: &ToplevelIdent) -> Result<LeaseId, DesktopError> {
        if self.held.values().any(|held| held == window) {
            return Err(DesktopError::Busy);
        }
        self.next += 1;
        let id = LeaseId(self.next);
        self.held.insert(id, window.clone());
        Ok(id)
    }

    /// Ends `lease`; an unknown lease is a window that is no longer there to give back.
    pub(crate) fn end(&mut self, lease: LeaseId) -> Result<(), DesktopError> {
        self.held
            .remove(&lease)
            .map(|_| ())
            .ok_or(DesktopError::NoSuchWindow)
    }

    /// `Ok` when `lease` is held.
    pub(crate) fn check(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.held
            .contains_key(&lease)
            .then_some(())
            .ok_or(DesktopError::NoSuchWindow)
    }

    /// Every lease, ended by the compositor going away.
    pub(crate) fn drain_revoked(&mut self) -> Vec<DesktopEvent> {
        std::mem::take(&mut self.held)
            .into_keys()
            .map(|lease| DesktopEvent::LeaseRevoked {
                lease,
                why: RevokeReason::Compositor,
            })
            .collect()
    }
}

/// Grants `req` on `window`: the in-place mode on a stock COSMIC desktop, with the sender socket
/// of the shared seat. Every other mode, and every other backend, is refused: stock has no seat
/// for a hidden window (Spike A), and KWin's socket comes from the RemoteDesktop portal, which
/// no code here asks yet.
pub(crate) async fn grant(
    kind: BackendKind,
    sockets: Option<&dyn SenderSocket>,
    leases: &Mutex<Leases>,
    req: LeaseRequest,
    window: ToplevelInfo,
) -> Result<Leased, DesktopError> {
    match (kind, req.mode) {
        (BackendKind::CosmicStock, RunMode::InPlace) => {}
        (_, mode) => return Err(DesktopError::ModeUnsupported(mode)),
    }
    let sockets = sockets.ok_or_else(|| DesktopError::Protocol("no EIS socket source".into()))?;
    let id = leases
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take(&req.window)?;
    match sockets.sender_socket(DeviceTypes::ALL).await {
        Ok(fd) => Ok(Leased {
            grant: LeaseGrant {
                id,
                window,
                seat: SeatIsolation::SharedSeat,
                ei: EiOrigin::SenderSocket,
            },
            socket: EiSocket(fd),
        }),
        Err(error) => {
            // Nothing was granted: the window is free again.
            let _ = leases
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .end(id);
            Err(error)
        }
    }
}
