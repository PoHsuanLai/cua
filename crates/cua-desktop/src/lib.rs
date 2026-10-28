//! The desktop vocabulary every cua crate shares, and the seams cuad reaches the desktop
//! through.
//!
//! It is the base of the repo: windows ([`ToplevelInfo`]), leases ([`LeaseRequest`],
//! [`LeaseGrant`], [`Leased`]), what a backend can do ([`BackendCaps`]), what the glow shows
//! ([`GlowState`]), what the desktop reports ([`DesktopEvent`]), and the units the other cua
//! crates share ([`Stamp`], [`Ms`], [`Digest32`]).
//! `RunMode` and `WindowTrust` live in stoker's `cua-action` (with `WindowClass`) and are
//! re-exported here.
//!
//! Pure: no backend lives here. A compositor backend implements [`Desktop`] and
//! [`LeaseControl`].

mod caps;
mod error;
mod event;
mod glow;
mod lease;
mod traits;
mod units;
mod window;

pub use caps::{BackendCaps, BackendKind, ConsentGuard, HiddenCapture, MaskWhere};
pub use cua_action::{RunMode, WindowTrust};
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
