//! cuad on the bus.

use crate::supervisor::Supervisor;
use cua_run::Seams;

/// Why the daemon stopped.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The session bus could not be reached or the name could not be taken.
    #[error("bus: {0}")]
    Bus(String),
}

/// Serves `org.quire.Cua1` (`Manager` at `/org/quire/Cua1`, a `Run` per live run at
/// `/org/quire/Cua1/run/<id>`) and `org.quire.IntentProvider1` as `org.quire.Cua`, with each
/// caller's role derived from the connection, until the bus goes away.
pub async fn serve<S: Seams>(supervisor: Supervisor<S>) -> Result<(), ServeError> {
    let _ = supervisor;
    todo!(
        "one handler per Cua1 member over Supervisor, caller_of the connection, signals from CuaSignal, the provider through docket-client serve"
    )
}
