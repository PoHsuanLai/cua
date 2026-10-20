//! The desktop and the accessibility source of the running daemon, each with the case where
//! there is nothing to reach: cuad still starts, answers the bus, and says what is missing.

use crate::casement::CasementDesktop;
use a11y_tree::{A11yError, A11ySnapshot, A11ySource, A11yTarget, AtspiSource, NodeBudget};
use cua_desktop::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, GlowState, LeaseControl,
    LeaseId, LeaseRequest, Leased, Suspend, ToplevelInfo,
};

/// The compositor's desktop, or none. A compositor backend is a variant added beside `Absent`.
#[derive(Debug)]
pub enum SystemDesktop {
    /// No compositor backend is built: `Backend` reads `none`, no window is listed and no lease
    /// is granted.
    Absent,
    /// casement: windows from its desktop protocols and a lease per window over `Compositor1`.
    Casement(CasementDesktop),
}

fn nobody() -> DesktopError {
    DesktopError::Protocol("no compositor".into())
}

impl Desktop for SystemDesktop {
    fn caps(&self) -> BackendCaps {
        match self {
            // Never offered (`backend` is `None`); a value is needed all the same.
            SystemDesktop::Absent => BackendCaps::of(BackendKind::KwinStock),
            SystemDesktop::Casement(desktop) => desktop.caps(),
        }
    }

    fn backend(&self) -> Option<BackendKind> {
        match self {
            SystemDesktop::Absent => None,
            SystemDesktop::Casement(desktop) => desktop.backend(),
        }
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        match self {
            SystemDesktop::Absent => Err(nobody()),
            SystemDesktop::Casement(desktop) => desktop.windows().await,
        }
    }

    async fn next_event(&self) -> DesktopEvent {
        match self {
            SystemDesktop::Absent => std::future::pending().await,
            SystemDesktop::Casement(desktop) => desktop.next_event().await,
        }
    }
}

impl LeaseControl for SystemDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        match self {
            SystemDesktop::Absent => Err(nobody()),
            SystemDesktop::Casement(desktop) => desktop.lease(req).await,
        }
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Absent => Ok(()),
            SystemDesktop::Casement(desktop) => desktop.release(lease).await,
        }
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
            SystemDesktop::Casement(desktop) => desktop.suspend(lease, why).await,
        }
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
            SystemDesktop::Casement(desktop) => desktop.resume(lease).await,
        }
    }

    async fn focus(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
            SystemDesktop::Casement(desktop) => desktop.focus(lease).await,
        }
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
            SystemDesktop::Casement(desktop) => desktop.glow(lease, glow).await,
        }
    }
}

/// The accessibility tree, or none.
#[derive(Debug)]
pub enum SystemA11y {
    /// No tree (`cuad.toml` `a11y = "off"`, the default until the AT-SPI source is built).
    Unavailable,
    /// AT-SPI.
    Atspi(AtspiSource),
}

impl A11ySource for SystemA11y {
    async fn snapshot(
        &self,
        target: &A11yTarget,
        budget: NodeBudget,
    ) -> Result<A11ySnapshot, A11yError> {
        match self {
            SystemA11y::Unavailable => Err(A11yError::NoBus),
            SystemA11y::Atspi(source) => source.snapshot(target, budget).await,
        }
    }
}
