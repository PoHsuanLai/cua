//! The seam cuad injects input through.

use crate::caps::SeatCaps;
use crate::keymap::Keymap;
use crate::op::InputOp;
use crate::receipt::InputReceipt;
use cua_desktop::{EiSocket, GlobalRect, LeaseGrant, LeaseId};
pub use cua_types::SeatError;
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

    /// The leased window moved or was resized: pointer points map to `frame` from now on, and
    /// the pointer is held inside it. A seat whose region is the window itself (the fork's) has
    /// nothing to do.
    fn rebind(
        &self,
        _lease: LeaseId,
        _frame: GlobalRect,
    ) -> impl Future<Output = Result<(), SeatError>> + Send {
        async { Ok(()) }
    }

    /// Stops emulating and closes the connection.
    fn detach(&self, lease: LeaseId) -> impl Future<Output = Result<(), SeatError>> + Send;
}
