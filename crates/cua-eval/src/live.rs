//! A real computer-use model, reached the way inferd reaches one: stoker's `CuaSession` builds
//! each step's prompt from the model's catalogue entry, a provider (an engine on this machine,
//! over loopback HTTP) runs the turn, and the session reads the reply into actions. This is the
//! same turn inferd makes, without inferd, so the eval needs no daemon and no bus.
//!
//! The entry is read from a file the caller names (stoker's catalogue); nothing here chooses a
//! model, an engine or an address.

use cua_action::{CuaDialect, Size, WindowSpace};
use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepEvent, StepSink, ThoughtText};
use cua_session::{
    CuaProfile, CuaSession, CuaTaskText, FrameBudget, MaskedRegions, ObservationIn, RepairBudget,
    StepIndex, StepLines, StepNote as NoteLine, StepOutcome, TranscriptSink,
    TreeText as WindowText, TurnSettings,
};
use cua_vendors::StepResult;
use model_catalog::{ModelEntry, parse_entry};
use model_provider::{
    Batching, CuaSupport, EngineExtras, ImageBytes, ImageDetail, ImageInput, Limits, ModelName,
    Provider, ProviderError, Reasoning, Sampling, ToolCallId, ToolParallelism,
};
use porter_core::Tokens;
use porter_infer::{
    CuaBegin, CuaStepReply, CuaStepRequest, DropReason, DroppedAction, NoteFrom, PrevResult,
    ServedBy, StepNote, TokenUsage, TreeText,
};
use std::sync::Arc;
use vision_prep::{Encoding, FrameMap, RawFrame, prepare};

/// Why a model could not be set up.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiveError {
    /// The entry file is unreadable or not a catalogue entry.
    #[error("catalogue entry: {0}")]
    Entry(String),
    /// The entry is not a computer-use model with a tool or text dialect.
    #[error("the entry is not a computer-use model this eval can drive")]
    NotComputerUse,
}

/// Whether the model thinks before it acts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Thinking {
    /// No: the entry's greedy sampling, the same screen gives the same click.
    #[default]
    Off,
    /// Yes: the entry's thinking sampling.
    On,
}

/// What the entry says about driving the model.
#[derive(Debug, Clone)]
pub struct Plan {
    profile: CuaProfile,
    settings: TurnSettings,
    name: ModelName,
}

impl Plan {
    /// The plan for the entry in `text`, served by the engine under `served_name`.
    pub fn of(text: &str, served_name: &str, thinking: Thinking) -> Result<Plan, LiveError> {
        let entry: ModelEntry = parse_entry(text).map_err(|e| LiveError::Entry(e.to_string()))?;
        Plan::from_entry(&entry, served_name, thinking)
    }

    fn from_entry(
        entry: &ModelEntry,
        served_name: &str,
        thinking: Thinking,
    ) -> Result<Plan, LiveError> {
        let caps = &entry.capabilities;
        let images = caps.image_in.ok_or(LiveError::NotComputerUse)?;
        let text_out = caps.text_out.as_ref().ok_or(LiveError::NotComputerUse)?;
        let sampling = text_out.sampling.ok_or(LiveError::NotComputerUse)?;
        let CuaSupport::Dialect {
            dialect, batching, ..
        } = caps.actions_out.ok_or(LiveError::NotComputerUse)?
        else {
            return Err(LiveError::NotComputerUse);
        };
        if matches!(dialect, CuaDialect::Wire(_)) {
            return Err(LiveError::NotComputerUse);
        }
        let profile = CuaProfile::for_model(
            dialect,
            &images,
            FrameBudget::within(images.per_prompt),
            RepairBudget(1),
            Encoding::Png,
        );
        let (sampling, reasoning): (Sampling, Reasoning) = match thinking {
            Thinking::Off => (sampling.reasoning_off, Reasoning::Off),
            Thinking::On => (sampling.reasoning_on, Reasoning::EngineDefault),
        };
        let settings = TurnSettings {
            sampling,
            limits: Limits {
                max_output: text_out.max_output,
                stop: Vec::new(),
            },
            reasoning,
            tool_calls: match batching {
                Batching::One => ToolParallelism::One,
                Batching::Many => ToolParallelism::Many,
            },
            engine: EngineExtras::None,
            lines: StepLines(8),
        };
        Ok(Plan {
            profile,
            settings,
            name: ModelName(served_name.to_owned()),
        })
    }
}

/// A [`CuaModel`] over a provider.
#[derive(Debug)]
pub struct ProviderModel<P> {
    provider: Arc<P>,
    plan: Plan,
    session: Option<CuaSession>,
    served: ServedBy,
    usage: TokenUsage,
}

impl<P: Provider> ProviderModel<P> {
    /// A model that runs `plan` on `provider`, saying it is `served` by this machine.
    pub fn new(provider: Arc<P>, plan: Plan, served: ServedBy) -> ProviderModel<P> {
        ProviderModel {
            provider,
            plan,
            session: None,
            served,
            usage: TokenUsage {
                input: Tokens(0),
                output: Tokens(0),
                cached: Tokens(0),
            },
        }
    }
}

/// What a turn said, whole: its thinking, its words and its calls.
fn said_in(turn: &cua_session::TurnTranscript) -> String {
    let calls: Vec<&str> = turn.calls.iter().map(|c| c.input.as_str()).collect();
    format!("{} {} {}", turn.thought, turn.text, calls.join(" "))
}

fn model_error(error: &ProviderError) -> CuaModelError {
    match error {
        ProviderError::Unreachable
        | ProviderError::NotReady
        | ProviderError::Timeout
        | ProviderError::RateLimited(_)
        | ProviderError::Server(_) => CuaModelError::Transient,
        ProviderError::Unreadable(_) => CuaModelError::Parse,
        _ => CuaModelError::Fatal,
    }
}

fn prev_results(prev: &[PrevResult]) -> Vec<StepResult> {
    prev.iter()
        .enumerate()
        .map(|(index, result)| {
            let id = ToolCallId(format!("prev-{index}"));
            match result {
                PrevResult::Done => StepResult::Done(id),
                PrevResult::NotRun => StepResult::NotRun(id),
                PrevResult::Refused(why) | PrevResult::Failed(why) => StepResult::Refused {
                    id,
                    why: why.clone(),
                },
                PrevResult::UserDeclined => StepResult::Refused {
                    id,
                    why: "the person declined it".to_owned(),
                },
                PrevResult::UserActed => StepResult::Refused {
                    id,
                    why: "the person did it themselves".to_owned(),
                },
            }
        })
        .collect()
}

fn note_line(note: &StepNote) -> NoteLine {
    let who = match note.from {
        NoteFrom::Person => "The person says",
        NoteFrom::Agent => "A helper says",
    };
    NoteLine(format!("{who}: {}", note.text))
}

fn observation(request: &CuaStepRequest) -> ObservationIn {
    let seen = ObservationIn::new(
        StepIndex(request.step.0),
        request.cursor,
        prev_results(&request.prev),
        MaskedRegions(request.masked.0),
    )
    .with_notes(request.notes.iter().map(note_line).collect());
    match &request.tree {
        TreeText::Present(tree) => seen.with_tree(WindowText(tree.clone())),
        TreeText::Absent => seen,
    }
}

fn dropped_reason(reason: cua_parse::DropReason) -> DropReason {
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

impl<P: Provider + 'static> CuaModel for ProviderModel<P> {
    async fn begin(&mut self, begin: CuaBegin) -> Result<(), CuaModelError> {
        let task = CuaTaskText {
            goal: begin.goal,
            hints: begin.hints,
        };
        let session = CuaSession::begin(self.plan.profile.clone(), task, self.plan.name.clone())
            .with_settings(self.plan.settings.clone());
        self.session = Some(session);
        Ok(())
    }

    async fn step(
        &mut self,
        request: CuaStepRequest,
        frame: FrameAttachment,
        sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        let session = self.session.as_mut().ok_or(CuaModelError::Fatal)?;
        let window: Size<WindowSpace> = request.window.logical;
        let map = FrameMap::new(
            window,
            request.window.scale,
            &self.plan.profile.rule,
            self.plan.profile.space,
        )
        .map_err(|_| CuaModelError::Fatal)?;
        let pixels = &frame.0;
        let raw = RawFrame {
            pixels: &pixels.data,
            size: pixels.size,
            stride: pixels.stride,
            format: pixels.format,
        };
        let prepared = prepare(raw, &map, Encoding::Png).map_err(|_| CuaModelError::Fatal)?;
        let image = ImageInput {
            media: prepared.media,
            bytes: ImageBytes(prepared.bytes),
            detail: ImageDetail::Auto,
        };
        let mut sent = session.request(&observation(&request), &map, image);
        loop {
            let mut collect = TranscriptSink::new();
            let end = match self.provider.turn(&sent, &mut collect).await {
                Ok(end) => end,
                Err(error) => {
                    session.refill_repairs();
                    return Err(model_error(&error));
                }
            };
            self.usage = TokenUsage {
                input: Tokens(end.usage.input.0),
                output: Tokens(end.usage.output.0),
                cached: Tokens(0),
            };
            let turn = collect.finish(end);
            sink.event(StepEvent::ThoughtDelta(ThoughtText(said_in(&turn))));
            match session.absorb_for(&sent, turn, &map) {
                StepOutcome::Actions {
                    thought,
                    actions,
                    dropped,
                } => {
                    return Ok(CuaStepReply {
                        thought,
                        actions,
                        dropped: dropped
                            .iter()
                            .map(|d| DroppedAction {
                                verb: d.verb.as_str().to_owned(),
                                reason: dropped_reason(d.reason),
                            })
                            .collect(),
                        safety: Vec::new(),
                    });
                }
                StepOutcome::Repair(next) => sent = next,
                StepOutcome::Unparseable(_) => return Err(CuaModelError::Parse),
            }
        }
    }

    fn usage(&self) -> TokenUsage {
        self.usage
    }

    fn served(&self) -> Option<ServedBy> {
        Some(self.served.clone())
    }
}
