//! The seam a run reaches its model through: inferd in cuad, a script in tests.

use crate::turn::FrameAttachment;
use cua_action::{CuaAction, WindowSpace};
use porter_infer::{
    CuaBegin, CuaStepFailure, CuaStepReply, CuaStepRequest, ModelError, ServedBy, TokenUsage,
};
use serde::{Deserialize, Serialize};
use std::future::Future;

/// What a model says while a step is still being decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum StepEvent {
    /// Some of the model's thought, as it streams (text of the model: `Debug` never prints it).
    ThoughtDelta(ThoughtText),
    /// An action the model proposed, in window space, before the batch is complete.
    ActionProposed(CuaAction<WindowSpace>),
}

/// A piece of a model's thought. `Debug` prints its length only.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThoughtText(pub String);

impl std::fmt::Debug for ThoughtText {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ThoughtText(<{} bytes>)", self.0.len())
    }
}

/// Where a step's events go while the model works.
pub trait StepSink: Send {
    /// One event.
    fn event(&mut self, event: StepEvent);
}

/// Why a model call failed, in the words the run machine counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CuaModelError {
    /// Worth retrying: the engine is busy, loading or unreachable.
    #[error("transient")]
    Transient,
    /// The reply could not be read even after inferd's one repair; counts as a failed step.
    #[error("unparseable reply")]
    Parse,
    /// Retrying will not help.
    #[error("fatal")]
    Fatal,
}

impl From<CuaStepFailure> for CuaModelError {
    /// inferd's failure, sorted into what the machine can act on: an unreachable, rate-limited
    /// or not-ready engine is transient; an unreadable reply is a parse failure; a refusal, an
    /// authorisation failure or a context overflow is fatal.
    fn from(failure: CuaStepFailure) -> Self {
        match failure {
            CuaStepFailure::Unparseable => CuaModelError::Parse,
            CuaStepFailure::ModelFailed(error) => match error {
                ModelError::Unreachable | ModelError::RateLimited(_) | ModelError::NotReady => {
                    CuaModelError::Transient
                }
                // A step that was all thinking chose no action: the same as one we could not read.
                ModelError::Unparseable
                | ModelError::Unreadable
                | ModelError::OnlyThought { .. } => CuaModelError::Parse,
                // Out of credit, or a sign-in the provider refused: no retry gets past either.
                ModelError::Unauthorized
                | ModelError::PaymentRequired
                | ModelError::SignInRefused
                | ModelError::Refused
                | ModelError::ContextOverflow => CuaModelError::Fatal,
            },
        }
    }
}

/// A computer-use model for one run.
pub trait CuaModel: Send {
    /// Starts the run's session with the goal and hints.
    fn begin(&mut self, begin: CuaBegin) -> impl Future<Output = Result<(), CuaModelError>> + Send;

    /// One step: the request, the frame beside it, and where to send events as they stream.
    fn step(
        &mut self,
        request: CuaStepRequest,
        frame: FrameAttachment,
        sink: &mut impl StepSink,
    ) -> impl Future<Output = Result<CuaStepReply, CuaModelError>> + Send;

    /// Tokens used by the last step.
    fn usage(&self) -> TokenUsage;

    /// Which account and model answered, once inferd has routed the session.
    fn served(&self) -> Option<ServedBy>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_infer::StopReason;

    #[test]
    fn inferd_failures_sort_into_transient_parse_and_fatal() {
        const CASES: &[(&str, CuaStepFailure, CuaModelError)] = &[
            (
                "unparseable",
                CuaStepFailure::Unparseable,
                CuaModelError::Parse,
            ),
            (
                "unreachable",
                CuaStepFailure::ModelFailed(ModelError::Unreachable),
                CuaModelError::Transient,
            ),
            (
                "rate limited",
                CuaStepFailure::ModelFailed(ModelError::RateLimited(5)),
                CuaModelError::Transient,
            ),
            (
                "not ready",
                CuaStepFailure::ModelFailed(ModelError::NotReady),
                CuaModelError::Transient,
            ),
            (
                "model unparseable",
                CuaStepFailure::ModelFailed(ModelError::Unparseable),
                CuaModelError::Parse,
            ),
            (
                "unreadable",
                CuaStepFailure::ModelFailed(ModelError::Unreadable),
                CuaModelError::Parse,
            ),
            (
                "only thought",
                CuaStepFailure::ModelFailed(ModelError::OnlyThought {
                    stop: StopReason::EndTurn,
                    thought_len: 12,
                }),
                CuaModelError::Parse,
            ),
            (
                "unauthorized",
                CuaStepFailure::ModelFailed(ModelError::Unauthorized),
                CuaModelError::Fatal,
            ),
            (
                "refused",
                CuaStepFailure::ModelFailed(ModelError::Refused),
                CuaModelError::Fatal,
            ),
            (
                "overflow",
                CuaStepFailure::ModelFailed(ModelError::ContextOverflow),
                CuaModelError::Fatal,
            ),
        ];
        for (name, failure, want) in CASES {
            assert_eq!(CuaModelError::from(*failure), *want, "{name}");
        }
    }

    #[test]
    fn thought_text_never_prints_the_thought() {
        let shown = format!(
            "{:?}",
            StepEvent::ThoughtDelta(ThoughtText("my secret plan".into()))
        );
        assert!(!shown.contains("secret"), "{shown}");
        assert!(shown.contains("<14 bytes>"), "{shown}");
    }
}
