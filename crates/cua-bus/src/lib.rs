//! What crosses cuad's bus and its event log, and nothing that runs.
//!
//! The `Cua1` bodies ([`CuaTask`], [`WindowBody`], [`BudgetBody`], [`AskBody`], [`RunEnd`]), run
//! states and outcomes ([`RunStateSlug`], [`CuaOutcome`], [`ModeSupport`]), the event-log record
//! ([`CuaRecord`]), the control refusals and who may call which member ([`ControlRefusal`],
//! [`permits`]). Serde only, so sill's models and the D-Bus codec read the same types.
//!
//! **One message model.** A run's progress and final result are `prov::Message`s of kind
//! `Report` (`Done`, `Failed`, `Cancelled`, `Progress`), sent to the requester through the
//! router: [`report_draft`] builds the draft, [`CuaOutcome::status`] picks the status. There is
//! no separate result type, and a report carries no authority: whatever it says is input to the
//! receiver, whose own task policy and the gate decide what follows.

mod action;
mod bodies;
mod budget;
mod control;
mod ids;
mod record;
mod report;
mod state;
mod task;

pub use action::{ActionOutcome, ActionResult, LocalRefusal, Refusal, TargetedAction};
pub use bodies::{AskBody, BudgetBody, WindowBody};
pub use budget::{BudgetUse, RunBudget, RunBudgetKind};
pub use control::{ControlRefusal, CuaCaller, CuaMember, permits};
pub use ids::{CuaEnvelope, CuaVocab, ProcedureId, QuestionId, WindowRef, slug_of};
pub use record::{
    AuthorisedBy, ConfirmBy, CuaRecord, FrameNote, ThoughtDigest, ThumbRef, cua_actor,
};
pub use report::{ReportDelivery, RunEnd, report_draft};
pub use state::{
    BlockedOn, CancelledBy, CuaOutcome, FailReason, ModeGap, ModeSupport, RunStateSlug,
    backend_wire, modes_wire,
};
pub use task::{CuaTask, ProcedureUse, ReportRoute, RunOrigin, SuccessCheck, TaskTarget};
