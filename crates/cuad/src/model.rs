//! The computer-use model over inferd.

use crate::frame_fd::frame_fd;
use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepEvent, StepSink, ThoughtText};
use porter_client::{Transport, TransportError};
use porter_core::capability::CuaEnv;
use porter_core::need::CuaNeed;
use porter_core::{DataClass, Need, Tier, Tokens};
use porter_infer::{
    AttachIndex, ClientFrame, CuaBegin, CuaStepFailure, CuaStepReply, CuaStepRequest, FrameLayout,
    ImageSource, InferEvent, InferRefusal, InferReply, InferRequest, InferSession, ServedBy,
    SessionError, TokenUsage,
};
use std::collections::BTreeSet;

/// A [`CuaModel`] over one inferd session (`Need::ComputerUse`, class `Screen`): `CuaBegin`
/// once, then a `CuaStep` per turn with the frame as a memfd attached to the session's fd,
/// never base64 on the bus. The route is chosen once, so the session is pinned to one model.
///
/// `begin` reads to the empty `CuaStepReply` that acknowledges it (the protocol has no other
/// reply for a begin; one with actions in it is a protocol fault). `step` forwards each
/// `ThoughtDelta` and `ActionProposed` to the sink as it arrives and ends on `Finished`. A model
/// failure ends the step and leaves the session usable (its history is intact); a session that
/// closes or breaks is dropped, and every later step is `Fatal`: the run cannot continue on
/// another model.
#[derive(Debug)]
pub struct InferdCuaModel<T: Transport> {
    transport: T,
    tier: Tier,
    session: Option<T::Session>,
    served: Option<ServedBy>,
    usage: Option<TokenUsage>,
}

impl<T: Transport> InferdCuaModel<T> {
    /// A model that has not opened its session yet; `begin` opens it, on the `Balanced` tier.
    pub fn over(transport: T) -> Self {
        Self {
            transport,
            tier: Tier::Balanced,
            session: None,
            served: None,
            usage: None,
        }
    }

    /// The tier the session is opened on (`CuadConfig::tier`).
    pub fn on_tier(self, tier: Tier) -> Self {
        Self { tier, ..self }
    }
}

/// The need a run of `env` opens its session with.
fn need_for(env: CuaEnv) -> Need {
    Need::ComputerUse(CuaNeed::new(BTreeSet::from([env])))
}

/// An unreachable inferd is worth retrying; a hung-up, confused or refusing one is not (a refused
/// caller stays refused until inferd's caller table names cuad).
fn transport_failure(error: TransportError) -> CuaModelError {
    match error {
        TransportError::Unreachable => CuaModelError::Transient,
        _ => CuaModelError::Fatal,
    }
}

/// Why a turn ended without the reply it was waiting for.
fn failed_turn(reply: InferReply) -> CuaModelError {
    match reply {
        InferReply::Failed(error) => CuaStepFailure::ModelFailed(error).into(),
        InferReply::Refused(InferRefusal::Unavailable) | InferReply::Cancelled => {
            CuaModelError::Transient
        }
        InferReply::Refused(_) => CuaModelError::Fatal,
        InferReply::CuaStep(_)
        | InferReply::Chat(_)
        | InferReply::Embed(_)
        | InferReply::Transcribed(_)
        | InferReply::Spoke(_) => CuaModelError::Fatal,
        // A reply this build does not know is never read as an answer.
        _ => CuaModelError::Fatal,
    }
}

fn zero() -> TokenUsage {
    TokenUsage {
        input: Tokens(0),
        output: Tokens(0),
        cached: Tokens(0),
    }
}

impl<T: Transport> CuaModel for InferdCuaModel<T> {
    async fn begin(&mut self, begin: CuaBegin) -> Result<(), CuaModelError> {
        let need = need_for(begin.env);
        let mut session = self
            .transport
            .open(&need, DataClass::Screen, self.tier)
            .await
            .map_err(transport_failure)?;
        session
            .send(ClientFrame::Request(InferRequest::CuaBegin(begin)))
            .await
            .map_err(|_| CuaModelError::Fatal)?;
        loop {
            match session.next().await.map_err(|_| CuaModelError::Fatal)? {
                InferEvent::Routed(served) => self.served = Some(served),
                InferEvent::Finished(InferReply::CuaStep(ack)) if is_ack(&ack) => break,
                InferEvent::Finished(other) => return Err(failed_turn(other)),
                _ => {}
            }
        }
        self.session = Some(session);
        Ok(())
    }

    async fn step(
        &mut self,
        mut request: CuaStepRequest,
        frame: FrameAttachment,
        sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        let session = self.session.as_mut().ok_or(CuaModelError::Fatal)?;
        // The bytes ride as the first descriptor, whatever the request says: the layout is the
        // buffer's own.
        request.frame.source = ImageSource::Attached(AttachIndex(0));
        request.frame.layout = FrameLayout::Raw {
            format: frame.0.format,
            size: frame.0.size,
            stride: frame.0.stride,
        };
        let fd = frame_fd(&frame).map_err(|_| CuaModelError::Fatal)?;
        self.usage = None;
        let sent = session
            .send_attached(
                ClientFrame::Request(InferRequest::CuaStep(request)),
                vec![fd],
            )
            .await;
        if let Err(error) = sent {
            return Err(self.lost(error));
        }
        loop {
            let event = match self.session.as_mut() {
                Some(session) => session.next().await,
                None => return Err(CuaModelError::Fatal),
            };
            match event {
                Ok(InferEvent::ThoughtDelta(text)) => {
                    sink.event(StepEvent::ThoughtDelta(ThoughtText(text)));
                }
                Ok(InferEvent::ActionProposed(action)) => {
                    sink.event(StepEvent::ActionProposed(action));
                }
                Ok(InferEvent::Routed(served)) => self.served = Some(served),
                Ok(InferEvent::Usage(usage)) => self.usage = Some(usage),
                Ok(InferEvent::Finished(InferReply::CuaStep(reply))) => return Ok(reply),
                Ok(InferEvent::Finished(other)) => return Err(failed_turn(other)),
                Ok(_) => {}
                Err(error) => return Err(self.lost(error)),
            }
        }
    }

    fn usage(&self) -> TokenUsage {
        self.usage.unwrap_or_else(zero)
    }

    fn served(&self) -> Option<ServedBy> {
        self.served.clone()
    }
}

impl<T: Transport> InferdCuaModel<T> {
    /// The session broke: it is gone, and so is the history the model was pinned to.
    fn lost(&mut self, _error: SessionError) -> CuaModelError {
        self.session = None;
        CuaModelError::Fatal
    }
}

/// The empty reply that acknowledges a begin.
fn is_ack(reply: &CuaStepReply) -> bool {
    reply.thought.is_none()
        && reply.actions.is_empty()
        && reply.dropped.is_empty()
        && reply.safety.is_empty()
}
