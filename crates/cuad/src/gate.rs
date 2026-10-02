//! The router's gate over `Intents1`: `Gate.Grant` and `Gate.Check`.

use cua_run::{GateCheck, GateEvent, GateLink, LinkError};
use docket_client::Transport;
use docket_core::{CuaAsk, GrantAnswer, GrantAsk};

/// `Intents1.Gate` over a docket transport (D-Bus in the daemon).
#[derive(Debug)]
pub struct IntentsGate<T: Transport> {
    transport: T,
}

impl<T: Transport> IntentsGate<T> {
    /// Calls the router through `transport`.
    pub fn over(transport: T) -> Self {
        Self { transport }
    }

    /// The transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }
}

/// One `Gate.Check` in flight: the Request object the router returned, whose `Progress` and
/// `Response` signals are the events.
#[derive(Debug)]
pub struct IntentsCheck {
    request: String,
}

impl IntentsCheck {
    /// The Request object's path.
    pub fn request(&self) -> &str {
        &self.request
    }
}

impl<T: Transport> GateLink for IntentsGate<T> {
    type Check = IntentsCheck;

    async fn grant(&self, ask: GrantAsk) -> Result<GrantAnswer, LinkError> {
        let _ = (ask, &self.transport);
        todo!(
            "IntentsRequest::GateGrant, wait for the Request's Response (the router asks the person on first use)"
        )
    }

    async fn check(&self, ask: CuaAsk) -> Result<IntentsCheck, LinkError> {
        let _ = ask;
        todo!("IntentsRequest::GateCheck; subscribe to the Request's Progress and Response signals")
    }
}

impl GateCheck for IntentsCheck {
    async fn next(&mut self) -> Result<GateEvent, LinkError> {
        todo!("Progress(Confirming(id)) becomes Confirming; Response becomes Answered(GateAnswer)")
    }

    async fn proceed(&mut self) -> Result<(), LinkError> {
        todo!("Request.Proceed on this request's object")
    }
}
