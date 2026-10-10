//! What the supervisor tells the bus layer, and what it knows of a live run.

use cua_bus::{BudgetBody, CuaTask, RunEnd};
use cua_run::{CuaSignal, RunPhase};
use prov::RunId;

/// What happened in the supervisor, in order: `serve` turns each into a D-Bus signal.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SupervisorEvent {
    /// A run began (`RunAdded`).
    Added(RunId),
    /// A run said something (`StateChanged`, `StepStarted`, ...).
    Signal(RunId, CuaSignal),
    /// A run ended; `Finished` carries how and which message carries its report.
    Ended(RunId, Box<RunEnd>),
    /// A run is no longer listed (`RunRemoved`).
    Removed(RunId),
}

/// A live run as the bus properties show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunView {
    /// Where it is.
    pub phase: RunPhase,
    /// What it was asked.
    pub task: CuaTask,
    /// The title of its window as last seen (screen text).
    pub title: String,
    /// Its limits and what it has used.
    pub budget: BudgetBody,
}
