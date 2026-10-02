//! The computer-use model over inferd.

use cua_run::{CuaModel, CuaModelError, FrameAttachment, StepSink};
use porter_client::Transport;
use porter_infer::{CuaBegin, CuaStepReply, CuaStepRequest, ServedBy, TokenUsage};

/// A [`CuaModel`] over one inferd session (`Need::ComputerUse`, class `Screen`): `CuaBegin`
/// once, then a `CuaStep` per turn with the frame as a memfd attached to the session's fd,
/// never base64 on the bus. The route is chosen once, so the session is pinned to one model.
#[derive(Debug)]
pub struct InferdCuaModel<T: Transport> {
    transport: T,
    session: Option<T::Session>,
    served: Option<ServedBy>,
    usage: Option<TokenUsage>,
}

impl<T: Transport> InferdCuaModel<T> {
    /// A model that has not opened its session yet; `begin` opens it.
    pub fn over(transport: T) -> Self {
        Self {
            transport,
            session: None,
            served: None,
            usage: None,
        }
    }
}

impl<T: Transport> CuaModel for InferdCuaModel<T> {
    async fn begin(&mut self, begin: CuaBegin) -> Result<(), CuaModelError> {
        let _ = (begin, &self.transport, &mut self.session);
        todo!(
            "Transport::open(Need::ComputerUse, Screen, tier), send ClientFrame::Request(CuaBegin), read Routed"
        )
    }

    async fn step(
        &mut self,
        request: CuaStepRequest,
        frame: FrameAttachment,
        sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        let _ = (request, frame, sink);
        todo!(
            "send CuaStep with the memfd; forward ThoughtDelta and ActionProposed to the sink; Finished(CuaStep) or CuaModelError::from(CuaStepFailure)"
        )
    }

    fn usage(&self) -> TokenUsage {
        self.usage.unwrap_or(TokenUsage {
            input: porter_core::Tokens(0),
            output: porter_core::Tokens(0),
            cached: porter_core::Tokens(0),
        })
    }

    fn served(&self) -> Option<ServedBy> {
        self.served.clone()
    }
}
