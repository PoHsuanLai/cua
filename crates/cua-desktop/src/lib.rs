//! The desktop vocabulary every cua crate shares, and the seams cuad reaches the desktop
//! through.
//!
//! It is the base of the repo: windows ([`ToplevelInfo`]), leases ([`LeaseRequest`],
//! [`LeaseGrant`], [`Leased`]), what a backend can do ([`BackendCaps`]), what the glow shows
//! ([`GlowState`]), what the desktop reports ([`DesktopEvent`]), and the units the other cua
//! crates share ([`Stamp`], [`Ms`], [`Digest32`]). `RunMode` and `WindowTrust` belong to
//! docket's gate contract and are re-exported here.
//!
//! Pure by default. The `wayland` feature is the compositor backend (stock cosmic-comp today,
//! casement's agent socket later) over wayland-client: [`CosmicDesktop`] reports the toplevels
//! and leases the stock in-place mode through a [`SenderSocket`] the caller supplies.

mod caps;
mod error;
mod event;
mod glow;
mod lease;
mod traits;
mod units;
mod window;

#[cfg(feature = "wayland")]
mod wayland;

pub use caps::{BackendCaps, BackendKind, ConsentGuard, GlowDraw, HiddenCapture, MaskWhere};
pub use docket_core::{RunMode, WindowTrust};
pub use error::DesktopError;
pub use event::DesktopEvent;
pub use glow::{GlowState, WireCode, mode_code};
pub use lease::{
    EiOrigin, EiSocket, LeaseGrant, LeaseId, LeaseRequest, Leased, ReleasePlacement, RevokeReason,
    SeatIsolation, Suspend,
};
pub use traits::{Desktop, LeaseControl};
pub use units::{Digest32, DigestError, GlobalCoord, GlobalRect, Ms, Pid, Stamp};
#[cfg(feature = "wayland")]
pub use wayland::{CosmicDesktop, DeviceTypes, Pump, PumpError, SenderSocket, SocketFuture};
pub use window::{
    IdentError, ToplevelIdent, ToplevelInfo, ToplevelState, WindowTitle, WorkspaceRef,
    WorkspaceShown,
};
