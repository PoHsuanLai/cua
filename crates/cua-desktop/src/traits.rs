//! The two seams cuad reaches the desktop through.

use crate::caps::{BackendCaps, BackendKind};
use crate::error::DesktopError;
use crate::event::DesktopEvent;
use crate::glow::GlowState;
use crate::lease::{LeaseId, LeaseRequest, Leased, Suspend};
use crate::window::ToplevelInfo;
use std::future::Future;

/// What is on the desktop.
pub trait Desktop: Send + Sync {
    /// What this backend can do.
    fn caps(&self) -> BackendCaps;

    /// The backend that is actually there: `caps().kind`, or `None` for a desktop that could not
    /// be reached at all (cuad then reports `Backend = none` and offers no mode).
    fn backend(&self) -> Option<BackendKind> {
        Some(self.caps().kind)
    }

    /// The toplevels now.
    fn windows(&self) -> impl Future<Output = Result<Vec<ToplevelInfo>, DesktopError>> + Send;

    /// The next change nobody asked for.
    fn next_event(&self) -> impl Future<Output = DesktopEvent> + Send;
}

/// Taking and giving back a window.
pub trait LeaseControl: Send + Sync {
    /// Leases a window: the grant and the EIS socket that goes with it.
    fn lease(&self, req: LeaseRequest)
    -> impl Future<Output = Result<Leased, DesktopError>> + Send;

    /// Ends a lease.
    fn release(&self, lease: LeaseId) -> impl Future<Output = Result<(), DesktopError>> + Send;

    /// Stops input without ending the lease.
    fn suspend(
        &self,
        lease: LeaseId,
        why: Suspend,
    ) -> impl Future<Output = Result<(), DesktopError>> + Send;

    /// Lets input flow again.
    fn resume(&self, lease: LeaseId) -> impl Future<Output = Result<(), DesktopError>> + Send;

    /// Asks the desktop to make the leased window the key window, so keys land in it. A
    /// backend that cannot (or that refuses: the lease is not live, the screen is locked, the
    /// window is in the dock) says [`DesktopError::Unsupported`] or its own error, and the run
    /// keeps its refusal of keys for a window that does not have the focus.
    fn focus(&self, lease: LeaseId) -> impl Future<Output = Result<(), DesktopError>> + Send {
        let _ = lease;
        async { Err(DesktopError::Unsupported) }
    }

    /// Shows what the run is doing.
    fn glow(
        &self,
        lease: LeaseId,
        glow: GlowState,
    ) -> impl Future<Output = Result<(), DesktopError>> + Send;
}

/// No desktop at all: `backend` reads `none`, no window is listed and no lease is granted.
impl<D: Desktop> Desktop for Option<D> {
    fn caps(&self) -> BackendCaps {
        // Never offered (`backend` is `None`); a value is needed all the same.
        self.as_ref()
            .map_or_else(|| BackendCaps::of(BackendKind::KwinStock), Desktop::caps)
    }

    fn backend(&self) -> Option<BackendKind> {
        self.as_ref().and_then(Desktop::backend)
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        match self {
            Some(desktop) => desktop.windows().await,
            None => Err(DesktopError::Protocol("no compositor".into())),
        }
    }

    async fn next_event(&self) -> DesktopEvent {
        match self {
            Some(desktop) => desktop.next_event().await,
            None => std::future::pending().await,
        }
    }
}

impl<D: LeaseControl> LeaseControl for Option<D> {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        match self {
            Some(desktop) => desktop.lease(req).await,
            None => Err(DesktopError::Protocol("no compositor".into())),
        }
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            Some(desktop) => desktop.release(lease).await,
            None => Ok(()),
        }
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        match self {
            Some(desktop) => desktop.suspend(lease, why).await,
            None => Err(DesktopError::NoSuchWindow),
        }
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            Some(desktop) => desktop.resume(lease).await,
            None => Err(DesktopError::NoSuchWindow),
        }
    }

    async fn focus(&self, lease: LeaseId) -> Result<(), DesktopError> {
        match self {
            Some(desktop) => desktop.focus(lease).await,
            None => Err(DesktopError::NoSuchWindow),
        }
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        match self {
            Some(desktop) => desktop.glow(lease, glow).await,
            None => Err(DesktopError::NoSuchWindow),
        }
    }
}
