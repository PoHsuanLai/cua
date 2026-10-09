//! The seam a snapshot comes through.

use crate::model::{A11ySnapshot, NodeBudget};
use cua_action::{Size, WindowSpace};
use cua_desktop::{Pid, WindowTitle};
use porter_core::AppName;
use serde::{Deserialize, Serialize};
use std::future::Future;

/// Which window's tree to take. The match is by process and title; it must be unambiguous.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct A11yTarget {
    /// The owning process.
    pub pid: Pid,
    /// The app.
    pub app: AppName,
    /// The window title.
    pub title: WindowTitle,
    /// The window's logical size, to tell windows of one process apart.
    pub geometry: Size<WindowSpace>,
}

/// Why no tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum A11yError {
    /// No accessibility bus.
    #[error("no accessibility bus")]
    NoBus,
    /// The app does not expose a tree.
    #[error("the app does not expose a tree")]
    NotExposed,
    /// No window matches the target, or several do.
    #[error("no unambiguous matching window")]
    NoMatchingWindow,
    /// Too slow.
    #[error("timed out")]
    Timeout,
    /// The window went away.
    #[error("the window is gone")]
    Gone,
    /// The bus's answer did not follow the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
    /// The backend is declared but not built yet.
    #[error("the accessibility backend is not built")]
    BackendUnavailable,
}

/// Takes accessibility snapshots.
pub trait A11ySource: Send + Sync {
    /// The tree of `target`, at most `budget` nodes.
    fn snapshot(
        &self,
        target: &A11yTarget,
        budget: NodeBudget,
    ) -> impl Future<Output = Result<A11ySnapshot, A11yError>> + Send;
}
