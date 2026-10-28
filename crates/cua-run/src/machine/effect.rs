//! What the machine wants done: every effect, and every signal the bus emits. cuad carries them
//! out; the machine never touches the world.

use crate::model::ThoughtText;
use crate::screen::ScreenText;
use crate::turn::FrameAttachment;
use a11y_tree::{NodeBudget, NodeSelector};
use cua_bus::{
    ActionOutcome, AskBody, CuaOutcome, CuaRecord, QuestionId, RunStateSlug, TargetedAction,
};
use cua_desktop::{GlowState, LeaseId, LeaseRequest, Ms, Suspend};
use docket_core::MessageDraft;
use docket_gate_wire::CuaAsk;
use maskcap::FrameWant;
use porter_infer::{CuaStepRequest, StepIndex};
use prov::SpaceId;
use seat_input::PlannedAction;
use serde::{Deserialize, Serialize};

/// Where the mask hints of a capture come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HintSource {
    /// The latest tree's secret rectangles and the sensitive-window rules.
    FromTree,
    /// None (the tree is off or absent).
    NoHints,
}

/// What the bus tells watchers (the `Cua1.Run` signals).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CuaSignal {
    /// The phase changed.
    StateChanged(RunStateSlug),
    /// A step began.
    StepStarted(StepIndex),
    /// Some of the model's thought.
    ThoughtDelta(StepIndex, ThoughtText),
    /// An action was proposed.
    ActionProposed(StepIndex, TargetedAction),
    /// An action ended.
    ActionDone(StepIndex, ActionOutcome),
    /// The model asked the person something.
    AskUser(QuestionId, AskBody),
    /// The run ended. The report to its requester is a `prov::Message` sent by
    /// `RunEffect::Report`.
    Finished(CuaOutcome),
}

/// One effect of the run machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RunEffect {
    /// `Gate.Grant`: the per-app consent for this app in this Space.
    Grant {
        /// The app.
        app: porter_core::AppName,
        /// The Space.
        space: SpaceId,
        /// The consent level the person set for the app.
        level: docket_permissions_core::Level,
    },
    /// Ask the desktop for the window.
    AcquireLease(LeaseRequest),
    /// Give the window back.
    ReleaseLease(LeaseId),
    /// Stop input (on stock, `AgentSeat::emulation(Stopped)` too).
    SuspendLease(LeaseId, Suspend),
    /// Let input flow.
    ResumeLease(LeaseId),
    /// Capture a frame.
    Capture {
        /// When.
        want: FrameWant,
        /// Mask hints.
        hints: HintSource,
    },
    /// Take an accessibility snapshot.
    Snapshot(NodeBudget),
    /// Ask the app for its `Context`.
    FetchContext,
    /// Ask the quarantined reader whether the screen's text is aimed at an assistant (only when
    /// the deterministic check found nothing and the reader is on). Answered with
    /// `RunInput::ScreenRead`.
    ReadScreen(ScreenText),
    /// Send the model its turn (the request, and the frame beside it).
    CallModel {
        /// The request.
        request: Box<CuaStepRequest>,
        /// The frame.
        frame: FrameAttachment,
    },
    /// Cancel the model call.
    CancelModel,
    /// `Gate.Check`: may the head action run?
    Check(Box<CuaAsk>),
    /// `Request.Proceed`: the lease is suspended, the router may show the confirmation.
    Proceed,
    /// Deliver input.
    Inject(PlannedAction),
    /// Watch the screen settle.
    WatchSettle {
        /// No change for this long means settled.
        quiet: Ms,
        /// Give up after this long.
        timeout: Ms,
    },
    /// Stop watching.
    CancelSettle,
    /// Check the success condition against the tree.
    VerifyTree(NodeSelector),
    /// Show what the run is doing.
    Glow(GlowState),
    /// Tell watchers.
    Emit(CuaSignal),
    /// Write to the event log.
    Record(Box<CuaRecord>),
    /// Speak to the requester: a draft of kind `Report` (Done, Failed, Cancelled or Progress).
    /// Sent through `Intents1.Message.Send`; the router stamps it.
    Report(Box<MessageDraft>),
}
