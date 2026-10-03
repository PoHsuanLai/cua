//! The router's gate over `Intents1`: `Gate.Grant` and `Gate.Check`, through docket-client's
//! `Intents::gate_grant` and `Intents::gate_check`.
//!
//! docket-client waits for the router's final answer inside one call: the confirmation of a
//! step is resolved inside the router before it answers, and its `Progress(Confirming)` is not
//! yet something the client hands out. So a check here ends in `Answered` and never says
//! `Confirming` first. That costs nothing the run relies on (cuad injects one action at a time
//! and only after the verdict, so its input is idle while the router asks), but the record of
//! the step cannot say the person confirmed it (FINDINGS, upstream asks).

use cua_run::{GateCheck, GateEvent, GateLink, LinkError};
use docket_client::{ClientError, Intents, Transport, TransportError};
use docket_core::{CuaAsk, GateAnswer, GrantAnswer, GrantAsk, IntentsReply, IntentsRequest};
use std::sync::Arc;
use tokio::task::JoinHandle;

/// `Intents1.Gate` over a docket transport (D-Bus in the daemon).
#[derive(Debug)]
pub struct IntentsGate<T: Transport> {
    transport: Arc<T>,
}

impl<T: Transport> IntentsGate<T> {
    /// Calls the router through `transport`.
    pub fn over(transport: T) -> Self {
        Self {
            transport: Arc::new(transport),
        }
    }

    /// The transport.
    pub fn transport(&self) -> &T {
        &self.transport
    }
}

/// A shared transport is a transport: each in-flight check holds its own handle.
#[derive(Debug)]
struct Shared<T: Transport>(Arc<T>);

impl<T: Transport> Transport for Shared<T> {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        self.0.call(request).await
    }
}

/// What a failed call means to a run: the router is not there, it refused the request outright,
/// or it did not follow the protocol.
fn link_error(error: ClientError) -> LinkError {
    match error {
        ClientError::Transport(TransportError::Closed) => LinkError::Unavailable,
        ClientError::Transport(other) => LinkError::Protocol(other.to_string()),
        ClientError::Refused(refusal) => LinkError::Refused(refusal),
        ClientError::Unexpected => LinkError::Protocol("unexpected reply".to_owned()),
    }
}

/// One `Gate.Check` in flight: the call runs on its own task, so the answer is read whenever
/// the run asks and the call is withdrawn if the run lets go of it.
#[derive(Debug)]
pub struct IntentsCheck {
    request: String,
    call: Option<JoinHandle<Result<GateAnswer, LinkError>>>,
}

impl IntentsCheck {
    /// The Request object's path, when the transport says it (docket-client does not yet: empty).
    pub fn request(&self) -> &str {
        &self.request
    }
}

impl Drop for IntentsCheck {
    fn drop(&mut self) {
        if let Some(call) = &self.call {
            call.abort();
        }
    }
}

impl<T: Transport + 'static> GateLink for IntentsGate<T> {
    type Check = IntentsCheck;

    async fn grant(&self, ask: GrantAsk) -> Result<GrantAnswer, LinkError> {
        Intents::over(Shared(self.transport.clone()))
            .gate_grant(ask)
            .await
            .map_err(link_error)
    }

    async fn check(&self, ask: CuaAsk) -> Result<IntentsCheck, LinkError> {
        let intents = Intents::over(Shared(self.transport.clone()));
        let call = tokio::spawn(async move { intents.gate_check(ask).await.map_err(link_error) });
        Ok(IntentsCheck {
            request: String::new(),
            call: Some(call),
        })
    }
}

impl GateCheck for IntentsCheck {
    async fn next(&mut self) -> Result<GateEvent, LinkError> {
        let Some(call) = self.call.as_mut() else {
            return Err(LinkError::Protocol(
                "the check was already answered".to_owned(),
            ));
        };
        // Polled by reference: a caller that stops waiting loses nothing, and asks again.
        let joined = call.await;
        self.call = None;
        match joined {
            Ok(answer) => answer.map(GateEvent::Answered),
            Err(_) => Err(LinkError::Unavailable),
        }
    }

    async fn proceed(&mut self) -> Result<(), LinkError> {
        // The router resolves its confirmation inside the call; there is nothing to release.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{CuaAction, WindowSpace};
    use docket_core::{CallRefusal, ConfirmEnd, EffectBasis, RunMode, WindowTrust, WireRefusal};
    use prov::{Effect, Label, RunId, SpaceId};
    use std::collections::VecDeque;
    use std::sync::Mutex;

    /// A router that says what it was told to say, and remembers what it was asked.
    struct Scripted {
        replies: Mutex<VecDeque<Result<IntentsReply, TransportError>>>,
        asked: Mutex<Vec<IntentsRequest>>,
    }

    impl Scripted {
        fn saying(replies: Vec<Result<IntentsReply, TransportError>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    impl Transport for Scripted {
        async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
            self.asked.lock().expect("lock").push(request);
            self.replies
                .lock()
                .expect("lock")
                .pop_front()
                .unwrap_or(Err(TransportError::Closed))
        }
    }

    fn grant_ask() -> GrantAsk {
        GrantAsk {
            app: porter_core::AppName::parse("org.quire.Mail").unwrap(),
            space: SpaceId::parse("work").unwrap(),
        }
    }

    fn cua_ask() -> CuaAsk {
        CuaAsk {
            run: RunId::parse("r-1").unwrap(),
            step: 1,
            app: porter_core::AppName::parse("org.quire.Mail").unwrap(),
            trust: WindowTrust::Quire,
            mode: RunMode::InPlace,
            space: SpaceId::parse("work").unwrap(),
            action: CuaAction::<WindowSpace>::Observe,
            node: None,
            effect: Effect::Read,
            basis: EffectBasis::DefaultTable,
            screen: Label::trusted_user(),
        }
    }

    #[tokio::test]
    async fn the_routers_answers_are_the_runs_answers() {
        let gate = IntentsGate::over(Scripted::saying(vec![
            Ok(IntentsReply::Granted(GrantAnswer::Refused(
                ConfirmEnd::Expired,
            ))),
            Ok(IntentsReply::Gate(GateAnswer::Refused(
                CallRefusal::Timeout,
            ))),
        ]));
        assert_eq!(
            gate.grant(grant_ask()).await,
            Ok(GrantAnswer::Refused(ConfirmEnd::Expired))
        );
        let mut check = gate.check(cua_ask()).await.expect("a check");
        assert_eq!(
            check.next().await,
            Ok(GateEvent::Answered(GateAnswer::Refused(
                CallRefusal::Timeout
            )))
        );
        let asked = gate.transport().asked.lock().expect("lock").clone();
        assert!(matches!(asked[0], IntentsRequest::GateGrant(_)));
        assert!(matches!(asked[1], IntentsRequest::GateCheck(_)));
    }

    #[tokio::test]
    async fn a_call_that_did_not_come_back_is_unavailable_or_a_protocol_fault() {
        type Row = (
            &'static str,
            Result<IntentsReply, TransportError>,
            LinkError,
        );
        let rows: Vec<Row> = vec![
            (
                "closed",
                Err(TransportError::Closed),
                LinkError::Unavailable,
            ),
            (
                "malformed",
                Err(TransportError::Malformed("x".into())),
                LinkError::Protocol("malformed message: x".into()),
            ),
            (
                "bus",
                Err(TransportError::Bus("y".into())),
                LinkError::Protocol("bus: y".into()),
            ),
            (
                "refused",
                Ok(IntentsReply::Refused(WireRefusal::NotAllowed)),
                LinkError::Refused(WireRefusal::NotAllowed),
            ),
            (
                "the wrong reply",
                Ok(IntentsReply::Done),
                LinkError::Protocol("unexpected reply".into()),
            ),
        ];
        for (name, reply, want) in rows {
            let gate = IntentsGate::over(Scripted::saying(vec![reply.clone(), reply]));
            assert_eq!(
                gate.grant(grant_ask()).await,
                Err(want.clone()),
                "{name}: grant"
            );
            let mut check = gate.check(cua_ask()).await.expect("a check starts");
            assert_eq!(check.next().await, Err(want), "{name}: check");
        }
    }

    #[tokio::test]
    async fn an_answered_check_has_nothing_more_to_say_and_proceed_is_a_no_op() {
        let gate = IntentsGate::over(Scripted::saying(vec![Ok(IntentsReply::Gate(
            GateAnswer::Run,
        ))]));
        let mut check = gate.check(cua_ask()).await.expect("a check");
        assert_eq!(check.request(), "");
        assert_eq!(check.proceed().await, Ok(()));
        assert_eq!(check.next().await, Ok(GateEvent::Answered(GateAnswer::Run)));
        assert!(matches!(check.next().await, Err(LinkError::Protocol(_))));
    }
}
