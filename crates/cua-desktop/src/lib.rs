//! The desktop vocabulary every cua crate shares, and the seams cuad reaches the desktop
//! through.
//!
//! It is the base of the repo: windows ([`ToplevelInfo`]), leases ([`LeaseRequest`],
//! [`LeaseGrant`], [`Leased`]), what a backend can do ([`BackendCaps`]), what the glow shows
//! ([`GlowState`]), what the desktop reports ([`DesktopEvent`]), and the units the other cua
//! crates share ([`Stamp`], [`Ms`], [`Digest32`]). `RunMode` and `WindowTrust` belong to
//! docket's gate contract and are re-exported here.
//!
//! Pure by default. The `cosmic` feature is the COSMIC backend (stock cosmic-comp and the quire
//! fork) over wayland-client and `quire-agent-protocol`; its bodies are `todo!()` until the
//! backend is built.

mod caps;
mod error;
mod event;
mod glow;
mod lease;
mod traits;
mod units;
mod window;

#[cfg(feature = "cosmic")]
mod cosmic;

pub use caps::{BackendCaps, BackendKind, ConsentGuard, GlowDraw, HiddenCapture, MaskWhere};
#[cfg(feature = "cosmic")]
pub use cosmic::CosmicDesktop;
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
pub use window::{
    IdentError, ToplevelIdent, ToplevelInfo, ToplevelState, WindowTitle, WorkspaceRef,
    WorkspaceShown,
};
