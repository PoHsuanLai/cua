//! The AT-SPI backend: the session's accessibility bus through the `atspi` crate. Chromium needs
//! `--force-renderer-accessibility`; quire apps expose their tree through `accesskit_unix`.

use crate::model::{A11ySnapshot, NodeBudget};
use crate::source::{A11yError, A11ySource, A11yTarget};

/// The accessibility bus, connected.
#[derive(Debug)]
pub struct AtspiSource {
    connection: Option<atspi::AccessibilityConnection>,
}

impl AtspiSource {
    /// A source that has not connected yet; the first snapshot connects.
    pub fn new() -> Self {
        Self { connection: None }
    }

    /// Whether a connection is held.
    pub fn is_connected(&self) -> bool {
        self.connection.is_some()
    }
}

impl Default for AtspiSource {
    fn default() -> Self {
        Self::new()
    }
}

impl A11ySource for AtspiSource {
    async fn snapshot(
        &self,
        target: &A11yTarget,
        budget: NodeBudget,
    ) -> Result<A11ySnapshot, A11yError> {
        // Not built: find the window by pid and title, walk the tree breadth-first to the
        // budget, parse roles, states, bounds (screen to window-local) and actions.
        let _ = (target, budget);
        Err(A11yError::BackendUnavailable)
    }
}
