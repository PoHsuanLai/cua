//! The compositor backend over a wayland connection: toplevels from `ext_foreign_toplevel_list_v1`
//! and `zcosmic_toplevel_info_v1`, workspaces from `ext_workspace_v1`, and the stock lease
//! (`GetSenderSocket`, reached through [`SenderSocket`] so this crate names no bus). One type
//! serves every [`BackendKind`] it was built with.
//!
//! The fork's own lease (`quire-agent-v1`, suspend, resume and glow in the compositor) is the
//! casement backend's (P4, after casement's agent socket): there is no [`BackendKind`] for it
//! yet, so nothing here branches on one. On stock, suspend, resume and glow are cuad's seat and
//! the shell's orb: nothing to say to the compositor.

mod key;
mod lease;
mod proto;
mod pump;
mod socket;
mod state;
mod toplevels;

pub use key::{ObjectKey, Rounds, SyncTag};
pub use pump::{Pump, PumpError};
pub use socket::{DeviceTypes, SenderSocket, SocketFuture};

use crate::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, GlowState, LeaseControl,
    LeaseId, LeaseRequest, Leased, Suspend, ToplevelInfo,
};
use lease::Leases;
use state::WlState;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use wayland_client::Connection;

/// Round trips that bring a fresh connection to rest: the registry, then the objects bound from
/// it, then the COSMIC handles made for the toplevels they list.
const SETTLE_ROUNDS: u32 = 3;

/// The desktop as the compositor reports it.
#[derive(Debug)]
pub struct CosmicDesktop {
    pump: Pump<WlState>,
    kind: BackendKind,
    sockets: Option<Arc<dyn SenderSocket>>,
    leases: Mutex<Leases>,
    lost: Mutex<VecDeque<DesktopEvent>>,
}

fn protocol(error: PumpError) -> DesktopError {
    DesktopError::Protocol(error.to_string())
}

impl CosmicDesktop {
    /// A desktop over an open connection (the caller connects, so tests pass a private
    /// socket). Nothing is asked of the compositor until the first call.
    pub fn over(connection: Connection, kind: BackendKind) -> Self {
        let queue = connection.new_event_queue::<WlState>();
        let state = WlState::new(&connection, queue.handle());
        Self {
            pump: Pump::new(connection, queue, state),
            kind,
            sockets: None,
            leases: Mutex::new(Leases::default()),
            lost: Mutex::new(VecDeque::new()),
        }
    }

    /// Where the EIS socket of a stock lease comes from.
    #[must_use]
    pub fn with_sender_socket(mut self, sockets: Arc<dyn SenderSocket>) -> Self {
        self.sockets = Some(sockets);
        self
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        self.pump.connection()
    }

    /// Brings the record up to date: every request made so far has been answered.
    async fn settle(&self) -> Result<(), DesktopError> {
        for _ in 0..SETTLE_ROUNDS {
            let mut asked = None;
            self.pump
                .drive(|st| {
                    let n = *asked.get_or_insert_with(|| st.sync());
                    st.synced(n).then_some(())
                })
                .await
                .map_err(protocol)?;
        }
        Ok(())
    }

    fn leases(&self) -> std::sync::MutexGuard<'_, Leases> {
        self.leases.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lost(&self) -> std::sync::MutexGuard<'_, VecDeque<DesktopEvent>> {
        self.lost.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Desktop for CosmicDesktop {
    fn caps(&self) -> BackendCaps {
        BackendCaps::of(self.kind)
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        self.settle().await?;
        self.pump
            .drive(|st| Some(toplevels::infos(st)))
            .await
            .map_err(protocol)?
    }

    async fn next_event(&self) -> DesktopEvent {
        loop {
            if let Some(event) = self.lost().pop_front() {
                return event;
            }
            match self.pump.drive(|st| st.events.pop_front()).await {
                Ok(event) => return event,
                Err(_) => {
                    // The connection is gone: every lease on it ended with it, once.
                    let ended = self.leases().drain_revoked();
                    if ended.is_empty() {
                        return std::future::pending().await;
                    }
                    self.lost().extend(ended);
                }
            }
        }
    }
}

impl LeaseControl for CosmicDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        let window = self.window(&req).await?;
        lease::grant(
            self.kind,
            self.sockets.as_deref(),
            &self.leases,
            req,
            window,
        )
        .await
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        // Giving back a lease that has already ended is not a failure.
        let _ = self.leases().end(lease);
        Ok(())
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        let _ = why;
        self.leases().check(lease)
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.leases().check(lease)
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        let _ = glow;
        self.leases().check(lease)
    }
}

impl CosmicDesktop {
    /// The toplevel `req` names, checked leasable.
    async fn window(&self, req: &LeaseRequest) -> Result<ToplevelInfo, DesktopError> {
        let windows = self.windows().await?;
        let info = windows
            .into_iter()
            .find(|w| w.ident == req.window)
            .ok_or(DesktopError::NoSuchWindow)?;
        req.check(&info)?;
        Ok(info)
    }
}

#[cfg(test)]
mod tests {
    use crate::glow::WireCode;
    use crate::{GlowState, RevokeReason, Suspend, mode_code};
    use casement_protocols::client::quire_agent_v1::quire_agent_lease_v1::{
        GlowState as ProtocolGlow, RevokeReason as ProtocolRevoke, SuspendReason as ProtocolSuspend,
    };
    use casement_protocols::client::quire_agent_v1::quire_agent_manager_v1::LeaseMode;

    #[test]
    fn the_wire_tables_equal_the_protocol_enums() {
        assert_eq!(GlowState::Off.code(), WireCode(ProtocolGlow::Off as u32));
        assert_eq!(
            GlowState::Working.code(),
            WireCode(ProtocolGlow::Working as u32)
        );
        let at = cua_action::Rect {
            origin: cua_action::Point::new(cua_action::Coord(0), cua_action::Coord(0)),
            size: cua_action::Size::new(cua_action::Coord(1), cua_action::Coord(1)),
        };
        assert_eq!(
            GlowState::Acting { at }.code(),
            WireCode(ProtocolGlow::Acting as u32)
        );
        assert_eq!(
            GlowState::WaitingForYou.code(),
            WireCode(ProtocolGlow::WaitingForYou as u32)
        );
        let revoke = [
            (RevokeReason::KillChord, ProtocolRevoke::KillChord as u32),
            (
                RevokeReason::WindowClosed,
                ProtocolRevoke::WindowClosed as u32,
            ),
            (
                RevokeReason::SpaceChanged,
                ProtocolRevoke::SpaceChanged as u32,
            ),
            (RevokeReason::Compositor, ProtocolRevoke::Compositor as u32),
        ];
        for (reason, want) in revoke {
            assert_eq!(reason.code(), WireCode(want), "{reason:?}");
        }
        let suspend = [
            (Suspend::TakeOver, ProtocolSuspend::TakeOver as u32),
            (Suspend::Consent, ProtocolSuspend::Consent as u32),
            (Suspend::Paused, ProtocolSuspend::Paused as u32),
        ];
        for (why, want) in suspend {
            assert_eq!(why.code(), WireCode(want), "{why:?}");
        }
        assert_eq!(
            mode_code(crate::RunMode::InPlace),
            WireCode(LeaseMode::InPlace as u32)
        );
        assert_eq!(
            mode_code(crate::RunMode::AgentWorkspace),
            WireCode(LeaseMode::AgentWorkspace as u32)
        );
        assert_eq!(
            mode_code(crate::RunMode::NestedSession),
            WireCode(LeaseMode::NestedSession as u32)
        );
    }
}
