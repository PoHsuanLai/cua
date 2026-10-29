//! A stoker [`Provider`] whose turns are inferd chat turns: one session per turn, opened as
//! `Need::Llm` (chat, vision, and tools when the turn has some) on data class `Screen`, the
//! request written with [`crate::request::chat`], the events read back into stoker's.
//!
//! inferd decides where the turn goes (the tier's row in its config), fetches the key from
//! accountd for that one turn and meters the reply against `[ai.spend]`. A turn refused on a
//! spend cap is counted in the [`Watch`] so the run can stop.

use crate::request::{RequestFault, SamplingFrom, chat};
use crate::watch::Watch;
use inferd_client::{OpenOptions, Transport, TransportError};
use model_provider as sp;
use model_provider::{Provider, ProviderError, TurnEvent, TurnSink};
use porter_capability::LlmFeature;
use porter_capability::Tier;
use porter_core::DataClass;
use porter_core::consent::Usage;
use porter_ids::Tokens;
use porter_infer::{
    ClientFrame, InferEvent, InferRefusal, InferReply, InferRequest, InferSession, ModelError,
    StopReason,
};
use porter_match::LlmNeed;
use porter_match::Need;
use std::collections::BTreeSet;
use std::sync::Arc;

/// A hosted model reached through inferd.
#[derive(Debug)]
pub struct InferdProvider<T> {
    transport: T,
    tier: Tier,
    sampling: SamplingFrom,
    watch: Arc<Watch>,
}

impl<T: Transport> InferdProvider<T> {
    /// A provider that asks inferd over `transport` at `tier`.
    pub fn new(transport: T, tier: Tier, sampling: SamplingFrom, watch: Arc<Watch>) -> Self {
        Self {
            transport,
            tier,
            sampling,
            watch,
        }
    }
}

/// What a chat turn with a screenshot (and tools, when there are some) needs of the model.
fn need(has_tools: bool) -> Need {
    let mut features = BTreeSet::from([LlmFeature::Chat, LlmFeature::Vision]);
    if has_tools {
        features.insert(LlmFeature::Tools);
    }
    Need::Llm(LlmNeed::new(features, Tokens(16_000)))
}

fn transport_fault(error: TransportError) -> ProviderError {
    match error {
        TransportError::Unreachable => ProviderError::Unreachable,
        TransportError::Closed | TransportError::Malformed(_) => {
            ProviderError::Unreadable("inferd closed or garbled the session".to_owned())
        }
        TransportError::Denied(why) => ProviderError::Refused(format!("inferd denied: {why}")),
        other => ProviderError::Unreadable(other.to_string()),
    }
}

fn model_fault(error: ModelError) -> ProviderError {
    match error {
        ModelError::Unreachable => ProviderError::Unreachable,
        ModelError::RateLimited(seconds) => ProviderError::RateLimited(sp::RetrySeconds(seconds)),
        ModelError::Unauthorized => ProviderError::Unauthorized,
        // Retrying cannot help either: the person tops up or picks another model, or the
        // provider has to accept the sign-in. The reason stays in inferd's logs, never here.
        ModelError::PaymentRequired => {
            ProviderError::PaymentRequired(sp::ProviderDetail::new(402, ""))
        }
        ModelError::SignInRefused => ProviderError::AuthRejected(sp::ProviderDetail::new(403, "")),
        ModelError::Refused => ProviderError::Refused(
            "the provider refused the request (OpenRouter answers 402 when max_tokens times the price exceeds the credit)"
                .to_owned(),
        ),
        ModelError::NotReady => ProviderError::NotReady,
        ModelError::ContextOverflow => ProviderError::ContextOverflow {
            limit: sp::Tokens(0),
        },
        other => ProviderError::Unreadable(other.to_string()),
    }
}

fn stop_of(stop: StopReason) -> sp::StopReason {
    match stop {
        StopReason::EndTurn => sp::StopReason::EndTurn,
        StopReason::ToolUse => sp::StopReason::ToolUse,
        StopReason::MaxTokens => sp::StopReason::MaxTokens,
        StopReason::StopSequence => sp::StopReason::StopSequence,
        StopReason::ContentFilter => sp::StopReason::ContentFilter,
        // A stop this build does not know is read as the provider withholding the reply.
        _ => sp::StopReason::ContentFilter,
    }
}

fn call_of(part: &porter_infer::ToolCallPart) -> Result<sp::ToolCall, ProviderError> {
    let unreadable = |what: &str| ProviderError::Unreadable(what.to_owned());
    Ok(sp::ToolCall {
        id: sp::ToolCallId(part.id.0.clone()),
        name: sp::ToolName::new(part.name.as_str()).map_err(|_| unreadable("a tool name"))?,
        input: sp::JsonText::new(part.args.as_str()).map_err(|_| unreadable("tool arguments"))?,
    })
}

fn fault(error: RequestFault) -> ProviderError {
    ProviderError::BadRequest(error.to_string())
}

/// Whether the turn's text and calls already arrived as events.
#[derive(Debug, Default)]
struct Seen {
    text: bool,
    calls: bool,
}

impl<T: Transport> Provider for InferdProvider<T> {
    async fn describe(&self) -> Result<Vec<sp::ModelInfo>, ProviderError> {
        Ok(Vec::new())
    }

    async fn turn<K: TurnSink>(
        &self,
        request: &sp::TurnRequest,
        sink: &mut K,
    ) -> Result<sp::TurnEnd, ProviderError> {
        let asked = chat(request, self.tier, self.sampling).map_err(fault)?;
        let options = OpenOptions::default().with_usage(Usage::Interactive);
        let mut session = self
            .transport
            .open_with(
                &need(!asked.tools.is_empty()),
                DataClass::Screen,
                self.tier,
                &options,
            )
            .await
            .map_err(transport_fault)?;
        session
            .send(ClientFrame::Request(InferRequest::Chat(asked)))
            .await
            .map_err(|e| ProviderError::Unreadable(e.to_string()))?;
        let mut seen = Seen::default();
        loop {
            let event = session
                .next()
                .await
                .map_err(|e| ProviderError::Unreadable(e.to_string()))?;
            match event {
                InferEvent::TextDelta(text) => {
                    seen.text = true;
                    sink.event(TurnEvent::TextDelta(text));
                }
                InferEvent::ThoughtDelta(text) => {
                    sink.event(TurnEvent::ThoughtDelta(text));
                }
                InferEvent::ToolCall(part) => {
                    seen.calls = true;
                    sink.event(TurnEvent::ToolCallDone(call_of(&part)?));
                }
                InferEvent::Finished(reply) => return self.finish(reply, seen, sink),
                _ => {}
            }
        }
    }
}

impl<T> InferdProvider<T> {
    fn finish<K: TurnSink>(
        &self,
        reply: InferReply,
        seen: Seen,
        sink: &mut K,
    ) -> Result<sp::TurnEnd, ProviderError> {
        match reply {
            InferReply::Chat(chat) => {
                if !seen.text && !chat.text.is_empty() {
                    sink.event(TurnEvent::TextDelta(chat.text.clone()));
                }
                if !seen.calls {
                    for part in &chat.tool_calls {
                        sink.event(TurnEvent::ToolCallDone(call_of(part)?));
                    }
                }
                self.watch.answered(
                    u64::from(chat.usage.input.0),
                    u64::from(chat.usage.output.0),
                    chat.usage.charge,
                );
                Ok(sp::TurnEnd {
                    stop: stop_of(chat.stop),
                    usage: sp::TurnUsage {
                        input: sp::Tokens(chat.usage.input.0),
                        output: sp::Tokens(chat.usage.output.0),
                        cached: sp::Tokens(chat.usage.cached.0),
                        images: sp::ImageCount(0),
                        charge: chat.usage.charge,
                    },
                    served: sp::ModelName(chat.served.model.to_string()),
                    first_token: None,
                })
            }
            InferReply::Refused(InferRefusal::OverBudget) => {
                self.watch.refuse_spend();
                Err(ProviderError::Refused("a spend cap was reached".to_owned()))
            }
            InferReply::Refused(why) => Err(ProviderError::Refused(why.to_string())),
            InferReply::Failed(error) => Err(model_fault(error)),
            InferReply::Cancelled => Err(ProviderError::Unreachable),
            other => Err(ProviderError::Unreadable(format!(
                "an unexpected reply: {other:?}"
            ))),
        }
    }
}
