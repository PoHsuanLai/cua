//! The COSMIC backend: toplevels from `ext_foreign_toplevel_list_v1` and
//! `zcosmic_toplevel_info_v1`, workspaces from `ext_workspace_v1`, and leases from
//! `quire-agent-v1` (fork) or the stock `GetSenderSocket` (stock). One type serves both; the
//! [`BackendKind`] it was built with decides which.

use crate::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, GlowState, LeaseControl,
    LeaseId, LeaseRequest, Leased, Suspend, ToplevelInfo,
};
use wayland_client::Connection;

/// The desktop as COSMIC reports it.
#[derive(Debug)]
pub struct CosmicDesktop {
    connection: Connection,
    kind: BackendKind,
}

impl CosmicDesktop {
    /// A desktop over an open connection (the caller connects, so tests pass a private
    /// socket).
    pub fn over(connection: Connection, kind: BackendKind) -> Self {
        Self { connection, kind }
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }
}

impl Desktop for CosmicDesktop {
    fn caps(&self) -> BackendCaps {
        BackendCaps::of(self.kind)
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        todo!(
            "bind the toplevel lists, create a cosmic handle per ext handle at creation, read geometry, state and workspace"
        )
    }

    async fn next_event(&self) -> DesktopEvent {
        todo!(
            "dispatch the connection: closed, geometry, lease revoked, physical input, kill chord"
        )
    }
}

impl LeaseControl for CosmicDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        let _ = req;
        todo!(
            "fork: create_lease and ei_socket; stock: GetSenderSocket with the CosmicOSK name workaround, refusing AgentWorkspace"
        )
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        let _ = lease;
        todo!("destroy the lease; restore the window per ReleasePlacement")
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        let _ = (lease, why);
        todo!("fork: lease.suspend(why); stock: stop emulating is cuad's seat, nothing here")
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        let _ = lease;
        todo!("fork: lease.resume")
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        let _ = (lease, glow);
        todo!("fork: lease.set_glow(code, rect); stock: nothing, the shell draws the orb")
    }
}

#[cfg(test)]
mod tests {
    use crate::glow::WireCode;
    use crate::{GlowState, RevokeReason, Suspend, mode_code};
    use quire_agent_protocol::client::quire_agent_lease_v1::{
        GlowState as ProtocolGlow, RevokeReason as ProtocolRevoke, SuspendReason as ProtocolSuspend,
    };
    use quire_agent_protocol::client::quire_agent_manager_v1::LeaseMode;

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
