//! What can happen to a run: every input of the machine. Each is `Serialize + Eq` so a run can
//! be written to a trace and replayed.

use crate::model::{CuaModelError, StepEvent};
use crate::observation::Observation;
use crate::screen::ScreenRead;
use crate::seams::LinkError;
use cua_bus::QuestionId;
use cua_desktop::{DesktopError, DesktopEvent, LeaseGrant};
use docket_core::{GateAnswer, GrantAnswer, InboundLine};
use maskcap::{CaptureError, DamageAt};
use porter_core::MicroUsd;
use porter_infer::{CuaStepReply, ServedBy, TokenUsage};
use prov::{ConfirmId, Labelled};
use seat_input::{InputReceipt, SeatError};
use serde::{Deserialize, Serialize};

/// What about the window changed between the look an action was aimed at and its injection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Moved {
    /// Another app or process owns the window now.
    Identity,
    /// The window's frame is not where it was.
    Geometry,
    /// The window gained or lost the keyboard focus.
    Focus,
}

/// A model's answer to one step, with what it cost.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelReply {
    /// The reply, in window space.
    pub reply: CuaStepReply,
    /// Tokens used.
    pub usage: TokenUsage,
    /// Money spent (zero on a local model).
    pub cost: MicroUsd,
}

/// The success check's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckResult {
    /// The condition holds.
    Pass,
    /// It does not.
    Fail,
}

/// One input of the run machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum RunInput {
    /// `Gate.Grant` ended.
    Granted(GrantAnswer),
    /// The router could not be asked.
    GrantFailed(LinkError),
    /// Nobody answered the per-app consent in time.
    GrantUnanswered,
    /// The desktop granted the lease.
    Leased(LeaseGrant),
    /// It did not.
    LeaseRefused(DesktopError),
    /// The step's observation is ready.
    Observed(Box<Observation>),
    /// The capture failed.
    CaptureFailed(CaptureError),
    /// inferd said who answers.
    Routed(ServedBy),
    /// The model said something while thinking.
    ModelEvent(StepEvent),
    /// The model answered.
    ModelReplied(Box<ModelReply>),
    /// The model failed.
    ModelFailed(CuaModelError),
    /// The model cannot read images; the sentence it gave for it, if any.
    ModelBlind(Option<String>),
    /// The quarantined reader's answer about the screen's text (`RunEffect::ReadScreen`).
    ScreenRead(ScreenRead),
    /// The router will ask the person about the head action (`Progress(Confirming)`).
    Confirming(ConfirmId),
    /// The router's answer for the head action.
    Verdict(GateAnswer),
    /// The router could not be asked.
    GateFailed(LinkError),
    /// Input was delivered.
    Injected(InputReceipt),
    /// Input could not be delivered.
    InjectFailed(SeatError),
    /// A record of an injected action could not be written. The run ends rather than go on
    /// with input nobody can account for.
    RecordFailed,
    /// The window is not what the action was aimed at: nothing was injected, and the run looks
    /// again.
    StateMoved(Moved),
    /// The screen changed.
    Damage(DamageAt),
    /// The success check ended.
    Checked(CheckResult),
    /// The person (or the planner or the shell) paused the run.
    UserPause,
    /// And resumed it.
    UserResume,
    /// And cancelled it.
    UserCancel,
    /// The person answered the model's question, in their own words.
    UserAnswer(QuestionId, Labelled<String>),
    /// The person took over the window.
    UserTakeOver,
    /// And handed it back.
    UserHandBack,
    /// A message arrived for the run. It is input like any other and widens nothing: no
    /// message grants a lease, a mode, a budget or a permission.
    Inbound(Box<InboundLine>),
    /// The desktop said something.
    Desktop(DesktopEvent),
    /// Time passed.
    Tick,
}
