//! The seam cuad injects input through.

use crate::caps::SeatCaps;
use crate::keymap::Keymap;
use crate::op::InputOp;
use crate::receipt::InputReceipt;
use cua_desktop::{EiSocket, LeaseGrant, LeaseId};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::sync::Arc;

/// Whether the seat is delivering input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emulation {
    /// Input flows (`start_emulating`).
    Running,
    /// Input is held back (`stop_emulating`): a confirmation is showing, or the run is paused.
    Stopped,
}

/// Why the seat could not deliver.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SeatError {
    /// No EIS connection.
    #[error("not connected")]
    NotConnected,
    /// The compositor refused.
    #[error("refused")]
    Refused,
    /// The operation lies outside the lease's window.
    #[error("outside the lease")]
    OutsideLease,
    /// The window is gone.
    #[error("the target is gone")]
    TargetGone,
    /// Emulation is stopped.
    #[error("suspended")]
    Suspended,
    /// The keymap could not be read.
    #[error("keymap: {0}")]
    Keymap(String),
    /// The EIS server's answer did not follow the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
}

/// The agent's hands.
pub trait AgentSeat: Send + Sync {
    /// What the seat offers.
    fn caps(&self) -> SeatCaps;

    /// The keymap the server sent on the latest `attach`; empty before the first.
    fn keymap(&self) -> Arc<Keymap>;

    /// Takes the EIS socket of a lease and starts emulating.
    fn attach(
        &self,
        lease: &LeaseGrant,
        socket: EiSocket,
    ) -> impl Future<Output = Result<(), SeatError>> + Send;

    /// Delivers `ops` in order.
    fn perform(
        &self,
        lease: LeaseId,
        ops: &[InputOp],
    ) -> impl Future<Output = Result<InputReceipt, SeatError>> + Send;

    /// Starts or stops delivery without ending the lease. On stock this is how cuad keeps its
    /// own input out of a confirmation.
    fn emulation(
        &self,
        lease: LeaseId,
        state: Emulation,
    ) -> impl Future<Output = Result<(), SeatError>> + Send;

    /// Stops emulating and closes the connection.
    fn detach(&self, lease: LeaseId) -> impl Future<Output = Result<(), SeatError>> + Send;
}
