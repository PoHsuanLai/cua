//! Why a desktop call failed.

use cua_action::{RunMode, WindowTrust};
use serde::{Deserialize, Serialize};

/// Why a desktop or lease call failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DesktopError {
    /// No such window (closed, or never leasable by that name).
    #[error("no such window")]
    NoSuchWindow,
    /// The window's trust class is never leased.
    #[error("a {0:?} window is not leasable")]
    NotLeasable(WindowTrust),
    /// The backend cannot run this mode.
    #[error("{0:?} is not supported here")]
    ModeUnsupported(RunMode),
    /// Another run holds the window or the seat.
    #[error("busy")]
    Busy,
    /// The compositor's answer did not follow the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
}
