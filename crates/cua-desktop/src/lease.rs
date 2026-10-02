//! A lease: the one window a run may see and act in, for as long as the run lasts.

use crate::error::DesktopError;
use crate::window::{ToplevelIdent, ToplevelInfo};
use docket_core::{RunMode, WindowTrust};
use prov::SpaceId;
use serde::{Deserialize, Serialize};
use std::os::fd::OwnedFd;

/// A lease, as cuad numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LeaseId(pub u64);

/// What a run asks of the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseRequest {
    /// The window.
    pub window: ToplevelIdent,
    /// Where the run happens.
    pub mode: RunMode,
    /// The Space the run acts in; the compositor refuses a window of another Space's workspace.
    pub space: SpaceId,
    /// Where the window goes when the lease ends.
    pub placement: ReleasePlacement,
}

/// Where a window goes when its lease ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleasePlacement {
    /// Back to where the person had it.
    ReturnToUser,
    /// Stays where the run left it.
    Stay,
}

/// How the EI sender context relates to the leased window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EiOrigin {
    /// The fork: bound to this lease's agent seat; its absolute region is the window.
    LeaseSocket,
    /// Stock: `com.system76.CosmicComp.Ei.GetSenderSocket`; the region is the output.
    SenderSocket,
}

/// The EIS socket cuad speaks to. The descriptor is not data: it travels beside the grant.
#[derive(Debug)]
pub struct EiSocket(pub OwnedFd);

/// What a lease grants: the window as it is now, and how input will reach it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeaseGrant {
    /// The lease.
    pub id: LeaseId,
    /// The window.
    pub window: ToplevelInfo,
    /// Whether the agent has a seat of its own.
    pub seat: SeatIsolation,
    /// Which kind of socket the descriptor is.
    pub ei: EiOrigin,
}

/// A grant and the EIS socket that goes with it.
#[derive(Debug)]
pub struct Leased {
    /// The grant.
    pub grant: LeaseGrant,
    /// The socket, for `AgentSeat::attach`.
    pub socket: EiSocket,
}

/// Whether the agent's input goes through a seat of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeatIsolation {
    /// The fork's agent seat: the person's cursor and focus are untouched.
    AgentSeat,
    /// Stock: the one shared seat. Input moves the person's cursor, steals focus, reaches
    /// layer-shell surfaces and triggers their shortcuts.
    SharedSeat,
}

/// Why input to a leased window stops without ending the lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Suspend {
    /// The person took over.
    TakeOver,
    /// A confirmation is showing; on stock, cuad stops emulating until it is answered.
    Consent,
    /// The run is paused.
    Paused,
}

/// Why a lease ended without cuad asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RevokeReason {
    /// The hardware kill chord.
    KillChord,
    /// The window closed.
    WindowClosed,
    /// The window left the run's Space.
    SpaceChanged,
    /// The compositor withdrew it.
    Compositor,
}

impl LeaseRequest {
    /// Whether this request may be granted for `info`: it names that window, and the window is
    /// not the shell's (a `Shell` window is never leasable, in any mode).
    pub fn check(&self, info: &ToplevelInfo) -> Result<(), DesktopError> {
        if info.ident != self.window {
            Err(DesktopError::NoSuchWindow)
        } else if info.trust == WindowTrust::Shell {
            Err(DesktopError::NotLeasable(WindowTrust::Shell))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{GlobalCoord, GlobalRect, Pid};
    use crate::window::{WindowTitle, WorkspaceRef, WorkspaceShown};
    use porter_core::{AppName, Px};
    use std::collections::BTreeSet;

    fn info(ident: &str, trust: WindowTrust) -> ToplevelInfo {
        ToplevelInfo {
            ident: ToplevelIdent::parse(ident).unwrap(),
            app: AppName::parse("org.gnome.Calculator").unwrap(),
            title: WindowTitle("Calculator".into()),
            pid: Pid(4242),
            geometry: GlobalRect {
                x: GlobalCoord(0),
                y: GlobalCoord(0),
                w: Px(400),
                h: Px(300),
            },
            workspace: WorkspaceRef {
                id: "ws-1".into(),
                shown: WorkspaceShown::Shown,
            },
            state: BTreeSet::new(),
            trust,
        }
    }

    fn request(ident: &str) -> LeaseRequest {
        LeaseRequest {
            window: ToplevelIdent::parse(ident).unwrap(),
            mode: RunMode::InPlace,
            space: SpaceId::parse("desktop").unwrap(),
            placement: ReleasePlacement::ReturnToUser,
        }
    }

    #[test]
    fn lease_request_refuses_shell_trust_and_wrong_windows() {
        let cases: Vec<(&str, &str, WindowTrust, Result<(), DesktopError>)> = vec![
            ("native window", "w1", WindowTrust::Native, Ok(())),
            ("quire window", "w1", WindowTrust::Quire, Ok(())),
            ("flatpak window", "w1", WindowTrust::Flatpak, Ok(())),
            (
                "the shell's own window",
                "w1",
                WindowTrust::Shell,
                Err(DesktopError::NotLeasable(WindowTrust::Shell)),
            ),
            (
                "another window",
                "w2",
                WindowTrust::Native,
                Err(DesktopError::NoSuchWindow),
            ),
        ];
        for (name, ident, trust, want) in cases {
            assert_eq!(request("w1").check(&info(ident, trust)), want, "{name}");
        }
    }
}
