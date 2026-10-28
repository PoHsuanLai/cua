//! The accessibility source of the running daemon. With no tree (`cuad.toml` `a11y = "off"`) the
//! seam is `None`, and cuad still starts, answers the bus, and says what is missing.

use a11y_atspi::AtspiSource;
use a11y_tree::{A11yError, A11ySnapshot, A11ySource, A11yTarget, NodeBudget};

/// AT-SPI, with one line logged per tree read.
#[derive(Debug)]
pub struct LoggedAtspi(pub AtspiSource);

impl A11ySource for LoggedAtspi {
    async fn snapshot(
        &self,
        target: &A11yTarget,
        budget: NodeBudget,
    ) -> Result<A11ySnapshot, A11yError> {
        let started = std::time::Instant::now();
        let snapshot = self.0.snapshot(target, budget).await;
        // One line per tree read: its size, or why a run goes on from pixels alone.
        let ms = started.elapsed().as_millis();
        match &snapshot {
            Ok(tree) => eprintln!(
                "cuad: accessibility tree: {} nodes in {ms} ms",
                tree.nodes.len()
            ),
            Err(error) => eprintln!("cuad: no accessibility tree after {ms} ms: {error:?}"),
        }
        snapshot
    }
}
