//! What a run writes to the event log. cuad stores each record as an almanac
//! `EventBody::Area { area: Cua }` whose `json` is the serde form below and whose kind tag is
//! the variant's slug (`cua.run.started`, ...). The actor is
//! `Actor::Companion { session, role: Cua { run } }` ([`cua_actor`]).
//!
//! No pixels and no thought text are stored: a thought becomes a digest, a frame a digest and a
//! size. The run's report to its requester is a `prov::Message`, stored by the router as an
//! `EventBody::Message`; [`CuaRecord::Finished`] links to it.

use crate::action::{ActionResult, TargetedAction};
use crate::budget::RunBudget;
use crate::ids::{ProcedureId, QuestionId};
use crate::state::CuaOutcome;
use cua_action::DeviceSize;
use cua_desktop::{BackendKind, Digest32};
use docket_core::{EffectBasis, RunMode};
use maskcap::FrameDigest;
use porter_core::{AppName, Count, MicroUsd, UnixSeconds};
use porter_infer::{ServedBy, StepIndex};
use prov::{Actor, AgentRole, Effect, Labelled, MessageId, RunId, SessionId, SpaceId};
use seat_input::InputReceipt;
use serde::{Deserialize, Serialize};

/// A digest of a model's thought: the text is never kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThoughtDigest(pub Digest32);

/// A stored thumbnail, in the Space's encrypted store.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThumbRef(pub String);

/// What is kept of a step's frame. `NotKept` is the default; `Kept` needs `cua.screens.keep`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum FrameNote {
    /// A digest and a size.
    NotKept {
        /// The frame's digest.
        digest: FrameDigest,
        /// Its size in device pixels.
        size: DeviceSize,
    },
    /// A thumbnail that expires.
    Kept {
        /// Where.
        thumb: ThumbRef,
        /// When it goes.
        expires: UnixSeconds,
    },
}

/// Who allowed an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorisedBy {
    /// The router's rulings, without asking.
    Policy,
    /// The person, in a confirmation.
    User,
}

/// How a confirmation was answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmBy {
    /// Hardware input on the compositor's own surface (the fork).
    PhysicalInput,
    /// The shell's sheet, after cuad stopped its input (stock).
    ShellPrompt,
}

/// One record of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v")]
pub enum CuaRecord {
    /// The run began.
    #[serde(rename = "cua.run.started")]
    RunStarted {
        /// The run.
        run: RunId,
        /// The session it acts for.
        session: SessionId,
        /// What it was asked.
        goal: Labelled<String>,
        /// The app of the leased window.
        app: AppName,
        /// The Space.
        space: SpaceId,
        /// The mode.
        mode: RunMode,
        /// The backend.
        backend: BackendKind,
        /// The limits.
        budget: RunBudget,
    },
    /// inferd said which account and model answers.
    #[serde(rename = "cua.run.routed")]
    Routed {
        /// The run.
        run: RunId,
        /// Who answers, and where it ran.
        served_by: ServedBy,
    },
    /// One action, decided and done.
    #[serde(rename = "cua.step")]
    Step {
        /// The run.
        run: RunId,
        /// Which step.
        n: StepIndex,
        /// The model's thought, as a digest.
        thought: ThoughtDigest,
        /// The action.
        action: TargetedAction,
        /// Its effect.
        effect: Effect,
        /// Why that effect.
        basis: EffectBasis,
        /// Who allowed it.
        authorised_by: AuthorisedBy,
        /// How it ended.
        result: ActionResult,
        /// The frame it was decided on.
        frame: FrameNote,
        /// What was injected, if anything.
        input: Option<InputReceipt>,
    },
    /// The model asked the person something.
    #[serde(rename = "cua.asked")]
    Asked {
        /// The run.
        run: RunId,
        /// Which step.
        n: StepIndex,
        /// Which question.
        question_id: QuestionId,
        /// The question, as screen text.
        question: Labelled<String>,
    },
    /// A confirmation was answered yes.
    #[serde(rename = "cua.confirmed")]
    Confirmed {
        /// The run.
        run: RunId,
        /// Which step.
        n: StepIndex,
        /// How.
        by: ConfirmBy,
    },
    /// The person took over the window.
    #[serde(rename = "cua.taken_over")]
    TakenOver {
        /// The run.
        run: RunId,
        /// Which step.
        n: StepIndex,
    },
    /// The person handed the window back.
    #[serde(rename = "cua.handed_back")]
    HandedBack {
        /// The run.
        run: RunId,
        /// Which step.
        n: StepIndex,
    },
    /// The run ended.
    #[serde(rename = "cua.run.finished")]
    Finished {
        /// The run.
        run: RunId,
        /// How.
        outcome: CuaOutcome,
        /// Steps taken.
        steps: Count,
        /// Money spent.
        cost: MicroUsd,
        /// The report message the router delivered, if it did.
        report: Option<MessageId>,
    },
    /// A procedure was learned (a later wave).
    #[serde(rename = "cua.procedure.learned")]
    ProcedureLearned {
        /// The procedure.
        procedure: ProcedureId,
        /// The run it came from.
        from: RunId,
    },
}

impl CuaRecord {
    /// The kind tag of this record: the serde tag, as stored.
    pub fn kind(&self) -> String {
        crate::slug_of(self).unwrap_or_default()
    }

    /// The run the record belongs to; none for a learned procedure's own row.
    pub fn run(&self) -> &RunId {
        match self {
            CuaRecord::RunStarted { run, .. }
            | CuaRecord::Routed { run, .. }
            | CuaRecord::Step { run, .. }
            | CuaRecord::Asked { run, .. }
            | CuaRecord::Confirmed { run, .. }
            | CuaRecord::TakenOver { run, .. }
            | CuaRecord::HandedBack { run, .. }
            | CuaRecord::Finished { run, .. } => run,
            CuaRecord::ProcedureLearned { from, .. } => from,
        }
    }
}

/// The actor of every record of a run, as the almanac log stores it.
pub fn cua_actor(session: SessionId, run: RunId) -> Actor {
    Actor::Companion {
        session,
        role: AgentRole::Cua { run },
    }
}
