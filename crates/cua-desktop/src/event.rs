//! What the desktop tells cuad on its own.

use crate::lease::{LeaseId, RevokeReason};
use crate::units::GlobalRect;
use crate::window::ToplevelIdent;
use serde::{Deserialize, Serialize};

/// A change cuad did not ask for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum DesktopEvent {
    /// A window closed.
    WindowClosed(ToplevelIdent),
    /// A window moved or resized.
    GeometryChanged(ToplevelIdent, GlobalRect),
    /// A lease ended without cuad releasing it.
    LeaseRevoked {
        /// The lease.
        lease: LeaseId,
        /// Why.
        why: RevokeReason,
    },
    /// The person used the keyboard or pointer while a lease on a shared seat was active.
    PhysicalInput {
        /// The lease.
        lease: LeaseId,
    },
    /// A confirmation surface appeared.
    ConsentShown,
    /// The confirmation surface went away.
    ConsentHidden,
    /// The hardware kill chord (fork).
    KillChord,
}
