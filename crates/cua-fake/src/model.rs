//! A model that says what it was scripted to say.

use cua_action::{CuaAction, ImageSpace, ModelSpace, ToolDialect, WindowSpace};
use cua_parse::{InSpace, ParseLimits, Parsed, parse_text, parse_tool_calls};
use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepSink};
use model_provider::{ProviderError, SafetySignal, ToolCall, TurnEvent};
use model_replay::{Cassette, Interaction};
use porter_core::Tokens;
use porter_infer::{
    CuaBegin, CuaStepReply, CuaStepRequest, DropReason, DroppedAction, SafetyHint, ServedBy,
    TokenUsage,
};
use std::collections::VecDeque;

/// Why a cassette could not become a script.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScriptError {
    /// A turn of the cassette is not a computer-use step.
    #[error("interaction {0} is not a computer-use step")]
    NotAStep(u32),
}

/// A [`CuaModel`] that answers each step with the next scripted reply or error, and records
/// what it was asked.
#[derive(Debug)]
pub struct ScriptedModel {
    script: VecDeque<Result<CuaStepReply, CuaModelError>>,
    served: Option<ServedBy>,
    began: Vec<CuaBegin>,
    requests: Vec<CuaStepRequest>,
}

impl ScriptedModel {
    /// A model that plays `script` in order; past its end every step fails `Fatal`.
    pub fn new(script: Vec<Result<CuaStepReply, CuaModelError>>) -> Self {
        Self {
            script: script.into(),
            served: None,
            began: Vec::new(),
            requests: Vec::new(),
        }
    }

    /// Says it is served by `served`.
    pub fn served_by(mut self, served: ServedBy) -> Self {
        self.served = Some(served);
        self
    }

    /// A script from a recorded cassette: each interaction's reply, parsed by stoker's
    /// `cua-parse` into window space.
    ///
    /// A cassette does not say how its model writes actions, so each reply is read as Qwen's
    /// tool call, then Holo 3.1's, then UI-TARS text, and the first reading that yields an
    /// action wins. Points are taken as window pixels (the replayed frames are the window at
    /// scale 1), so a recorded click lands where it was recorded. An interaction that ended in
    /// a provider error replays as that error; one that no dialect reads is `NotAStep`.
    pub fn from_cassette(cassette: &Cassette) -> Result<Self, ScriptError> {
        let script = cassette
            .interactions
            .iter()
            .map(|it| match &it.end {
                Err(error) => Ok(Err(model_error(error))),
                Ok(_) => reply_of(it).map(Ok),
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self::new(script))
    }

    /// The goals it was begun with.
    pub fn begun(&self) -> &[CuaBegin] {
        &self.began
    }

    /// The step requests it received.
    pub fn requests(&self) -> &[CuaStepRequest] {
        &self.requests
    }
}

impl CuaModel for ScriptedModel {
    async fn begin(&mut self, begin: CuaBegin) -> Result<(), CuaModelError> {
        self.began.push(begin);
        Ok(())
    }

    async fn step(
        &mut self,
        request: CuaStepRequest,
        frame: FrameAttachment,
        sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        let _ = (frame, sink);
        self.requests.push(request);
        self.script.pop_front().unwrap_or(Err(CuaModelError::Fatal))
    }

    fn usage(&self) -> TokenUsage {
        TokenUsage {
            input: Tokens(0),
            output: Tokens(0),
            cached: Tokens(0),
        }
    }

    fn served(&self) -> Option<ServedBy> {
        self.served.clone()
    }
}

/// How a provider's failure reaches the machine.
fn model_error(error: &ProviderError) -> CuaModelError {
    match error {
        ProviderError::Unreachable
        | ProviderError::NotReady
        | ProviderError::Timeout
        | ProviderError::RateLimited(_)
        | ProviderError::Server(_) => CuaModelError::Transient,
        ProviderError::Unreadable(_) => CuaModelError::Parse,
        ProviderError::Unauthorized
        | ProviderError::AuthRejected(_)
        | ProviderError::PaymentRequired(_)
        | ProviderError::ContextOverflow { .. }
        | ProviderError::BadRequest(_)
        | ProviderError::Refused(_) => CuaModelError::Fatal,
        // A failure a later stoker adds ends the run.
        _ => CuaModelError::Fatal,
    }
}

/// What an interaction's events add up to.
struct Turn {
    text: String,
    thought: String,
    calls: Vec<ToolCall>,
    safety: Vec<SafetyHint>,
}

fn turn_of(events: &[TurnEvent]) -> Turn {
    let mut turn = Turn {
        text: String::new(),
        thought: String::new(),
        calls: Vec::new(),
        safety: Vec::new(),
    };
    for event in events {
        match event {
            TurnEvent::TextDelta(t) => turn.text.push_str(t),
            TurnEvent::ThoughtDelta(t) => turn.thought.push_str(t),
            TurnEvent::ToolCallDone(call) => turn.calls.push(call.clone()),
            TurnEvent::Safety(SafetySignal::RequireConfirmation(why)) => {
                turn.safety
                    .push(SafetyHint::RequireConfirmation(why.clone()));
            }
            TurnEvent::Safety(SafetySignal::Blocked(why)) => {
                turn.safety.push(SafetyHint::Blocked(why.clone()));
            }
            _ => {}
        }
    }
    turn
}

fn parsed_of(turn: &Turn) -> Option<Parsed> {
    let limits = ParseLimits::default();
    let by_tool = [ToolDialect::QwenComputerUse, ToolDialect::Holo31]
        .into_iter()
        .filter(|_| !turn.calls.is_empty())
        .filter_map(|d| parse_tool_calls(d, ModelSpace::Image, &turn.calls, limits).ok());
    let by_text = parse_text(
        cua_action::TextDialect::UiTars15,
        ModelSpace::Image,
        &turn.text,
        limits,
    )
    .ok();
    by_tool
        .chain(by_text)
        .find(|p| !actions_of(&p.actions).is_empty())
}

fn actions_of(space: &InSpace) -> Vec<CuaAction<WindowSpace>> {
    let same = |p: cua_action::Point<ImageSpace>| {
        Ok::<_, std::convert::Infallible>(cua_action::Point::new(p.x, p.y))
    };
    let length = |l: cua_action::Length<ImageSpace>| {
        Ok::<_, std::convert::Infallible>(cua_action::Length::new(l.coord()))
    };
    match space {
        InSpace::Image(list) => list
            .iter()
            .cloned()
            .filter_map(|a| a.map_points(same, length).ok())
            .collect(),
        InSpace::Grid(..) => Vec::new(),
    }
}

fn reply_of(it: &Interaction) -> Result<CuaStepReply, ScriptError> {
    let turn = turn_of(&it.events);
    let parsed = parsed_of(&turn).ok_or(ScriptError::NotAStep(it.id.0))?;
    let thought = Some(turn.thought)
        .filter(|t| !t.is_empty())
        .or(parsed.thought.clone());
    let mut reply = CuaStepReply::new(actions_of(&parsed.actions))
        .with_dropped(
            parsed
                .dropped
                .iter()
                .map(|d| DroppedAction {
                    verb: d.verb.as_str().to_owned(),
                    reason: drop_reason(d.reason),
                })
                .collect(),
        )
        .with_safety(turn.safety);
    reply.thought = thought;
    Ok(reply)
}

fn drop_reason(reason: cua_parse::DropReason) -> DropReason {
    use cua_parse::DropReason as P;
    match reason {
        P::UnsupportedVerb => DropReason::UnsupportedVerb,
        P::MissingArgument => DropReason::MissingArgument,
        P::BadArgument => DropReason::BadArgument,
        P::BadNumber => DropReason::BadNumber,
        P::TooLong => DropReason::TooLong,
        P::OverBatchLimit => DropReason::OverBatchLimit,
        P::OutOfFrame => DropReason::OutOfFrame,
    }
}
