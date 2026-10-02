//! A model that says what it was scripted to say.

use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepSink};
use model_replay::Cassette;
use porter_core::Tokens;
use porter_infer::{CuaBegin, CuaStepReply, CuaStepRequest, ServedBy, TokenUsage};
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
    pub fn from_cassette(cassette: &Cassette) -> Result<Self, ScriptError> {
        let _ = cassette;
        todo!(
            "one CuaStepReply per interaction through cua-parse (the replay path of ScriptedModel)"
        )
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
