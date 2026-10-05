//! The desktop and the accessibility source of the running daemon, each with the case where
//! there is nothing to reach: cuad still starts, answers the bus, and says what is missing.

use a11y_tree::{A11yError, A11ySnapshot, A11ySource, A11yTarget, AtspiSource, NodeBudget};
use cua_desktop::{
    BackendCaps, BackendKind, CosmicDesktop, Desktop, DesktopError, DesktopEvent, GlowState,
    LeaseControl, LeaseId, LeaseRequest, Leased, Suspend, ToplevelInfo,
};

/// The compositor's desktop, or none.
#[derive(Debug)]
pub enum SystemDesktop {
    /// Connected to a compositor.
    Cosmic(CosmicDesktop),
    /// No compositor could be reached: `Backend` reads `none`, no window is listed and no lease
    /// is granted.
    Absent,
}

fn nobody() -> DesktopError {
    DesktopError::Protocol("no compositor".into())
}

impl Desktop for SystemDesktop {
    fn caps(&self) -> BackendCaps {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.caps(),
            // Never offered (`backend` is `None`); a value is needed all the same.
            SystemDesktop::Absent => BackendCaps::of(BackendKind::CosmicStock),
        }
    }

    fn backend(&self) -> Option<BackendKind> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.backend(),
            SystemDesktop::Absent => None,
        }
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.windows().await,
            SystemDesktop::Absent => Err(nobody()),
        }
    }

    async fn next_event(&self) -> DesktopEvent {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.next_event().await,
            SystemDesktop::Absent => std::future::pending().await,
        }
    }
}

impl LeaseControl for SystemDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.lease(req).await,
            SystemDesktop::Absent => Err(nobody()),
        }
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.release(lease).await,
            SystemDesktop::Absent => Ok(()),
        }
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.suspend(lease, why).await,
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
        }
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.resume(lease).await,
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
        }
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        match self {
            SystemDesktop::Cosmic(desktop) => desktop.glow(lease, glow).await,
            SystemDesktop::Absent => Err(DesktopError::NoSuchWindow),
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
