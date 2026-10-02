//! A window as the compositor reports it.

use crate::units::{GlobalRect, Pid};
use docket_core::WindowTrust;
use porter_core::AppName;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// The compositor's stable identifier of a toplevel (`ext_foreign_toplevel_handle_v1.identifier`).
/// Opaque: non-empty, at most 128 bytes, no control characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ToplevelIdent(String);

/// Why a string is not an identifier of ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an identifier is 1 to 128 bytes with no control characters")]
pub struct IdentError;

impl ToplevelIdent {
    /// Checks and wraps `text`.
    pub fn parse(text: &str) -> Result<Self, IdentError> {
        if text.is_empty() || text.len() > 128 || text.chars().any(char::is_control) {
            Err(IdentError)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    /// The identifier as the compositor spelled it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ToplevelIdent {
    type Error = IdentError;
    fn try_from(text: String) -> Result<Self, IdentError> {
        Self::parse(&text)
    }
}

impl From<ToplevelIdent> for String {
    fn from(ident: ToplevelIdent) -> String {
        ident.0
    }
}

/// A window title. It is screen text and third-party content: `Debug` prints its length only.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WindowTitle(pub String);

impl fmt::Debug for WindowTitle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "WindowTitle(<{} bytes>)", self.0.len())
    }
}

/// A workspace, by the compositor's identifier, and whether it is the one on screen.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkspaceRef {
    /// The compositor's identifier (`ext_workspace_handle_v1.id`).
    pub id: String,
    /// Whether it is showing.
    pub shown: WorkspaceShown,
}

/// Whether a workspace is the one on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceShown {
    /// Active on its output.
    Shown,
    /// Not on screen: stock cosmic-comp renders it for capture but delivers it no input.
    Hidden,
}

/// One bit of a toplevel's state (`zcosmic_toplevel_handle_v1.state`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToplevelState {
    /// Maximized.
    Maximized,
    /// Minimized: captured frames keep arriving, input does not.
    Minimized,
    /// The focused window of its seat.
    Activated,
}

/// A toplevel window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToplevelInfo {
    /// Its identifier.
    pub ident: ToplevelIdent,
    /// The app that owns it.
    pub app: AppName,
    /// Its title.
    pub title: WindowTitle,
    /// The process that owns it.
    pub pid: Pid,
    /// Its frame in global logical space.
    pub geometry: GlobalRect,
    /// Where it is.
    pub workspace: WorkspaceRef,
    /// Its state bits.
    pub state: BTreeSet<ToplevelState>,
    /// How far cua may trust it; `Shell` windows are never leasable.
    pub trust: WindowTrust,
}
