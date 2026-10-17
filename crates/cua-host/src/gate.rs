//! The router's gate over `Intents1`: `Gate.Grant`, `Session.Open` for the run's own session and
//! the watched `Gate.Check` (docket-client's `Intents::gate_check_watched`).
//!
//! A step's check says `Confirming(id)` when the router is about to ask the person: cuad stops
//! its own input (the run machine suspends the lease), calls `proceed`, and the sheet is drawn.
//! The verdict is the last event. A paused run takes its check back with `close`, and the sheet
//! comes down. A transport that cannot watch gives the verdict alone and the rest does nothing.

use cua_run::{GateCheck, GateEvent, GateLink, LinkError};
use docket_client::{
    ClientError, GateEvent as Heard, GateSteer, GateWatch, Intents, Transport, TransportError,
};
use docket_core::{CuaAsk, GrantAnswer, GrantAsk, IntentsReply, IntentsRequest, SessionOpen};
use prov::{AgentRef, RunId, SessionId, SpaceId};
use std::sync::Arc;

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

    async fn watch(
        &self,
        request: IntentsRequest,
    ) -> Result<docket_client::Watched, TransportError> {
        self.0.watch(request).await
    }
}

/// What a failed call means to a run: the router is not there, it refused the request outright,
/// or it did not follow the protocol.
fn link_error(error: ClientError) -> LinkError {
    match error {
        ClientError::Transport(TransportError::Closed) => LinkError::Unavailable,
        ClientError::Transport(other) => LinkError::Protocol(other.to_string()),
        ClientError::Refused(refusal) => LinkError::Refused(refusal),
        // Unexpected, or a failure a later docket adds: the run stops, never proceeds.
        _ => LinkError::Protocol("unexpected reply".to_owned()),
    }
}

/// One watched `Gate.Check` in flight. A check dropped before its verdict is withdrawn (its
/// sheet comes down), so a run that lets go of it never leaves the person a stale question.
pub struct IntentsCheck {
    watch: GateWatch,
    steer: GateSteer,
    over: Over,
}

/// Whether the router has had the last word on a check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Over {
    Open,
    Answered,
}

impl std::fmt::Debug for IntentsCheck {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IntentsCheck")
            .field("over", &self.over)
            .finish_non_exhaustive()
    }
}

impl Drop for IntentsCheck {
    fn drop(&mut self) {
        if self.over == Over::Open
            && let Ok(runtime) = tokio::runtime::Handle::try_current()
        {
            let steer = self.steer.clone();
            runtime.spawn(async move {
                let _ = steer.close().await;
            });
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

    async fn open_session(&self, run: &RunId, space: &SpaceId) -> Result<SessionId, LinkError> {
        let open = SessionOpen {
            space: space.clone(),
            agent: AgentRef::Cua { run: run.clone() },
            parent: None,
            // A computer-use run has no working directory, is no external agent, and was not
            // started from a terminal.
            cwd: None,
            external: None,
            helper: None,
            started_from: None,
        };
        Intents::over(Shared(self.transport.clone()))
            .session_open(open)
            .await
            .map(|opened| opened.session)
            .map_err(link_error)
    }

    async fn check(&self, ask: CuaAsk) -> Result<IntentsCheck, LinkError> {
        let watch = Intents::over(Shared(self.transport.clone()))
            .gate_check_watched(ask)
            .await
            .map_err(link_error)?;
        Ok(IntentsCheck {
            steer: watch.steer(),
            watch,
            over: Over::Open,
        })
    }
}

impl GateCheck for IntentsCheck {
    async fn next(&mut self) -> Result<GateEvent, LinkError> {
        if self.over == Over::Answered {
            return Err(LinkError::Protocol(
                "the check was already answered".to_owned(),
            ));
        }
        match self.watch.next().await {
            Ok(Heard::Confirming(id)) => Ok(GateEvent::Confirming(id)),
            Ok(Heard::Verdict(answer)) => {
                self.over = Over::Answered;
                Ok(GateEvent::Answered(answer))
            }
            Err(error) => {
                self.over = Over::Answered;
                Err(link_error(error))
            }
        }
    }

    async fn proceed(&mut self) -> Result<(), LinkError> {
        match self.over {
            Over::Answered => Ok(()),
            Over::Open => self.steer.proceed().await.map_err(link_error),
        }
    }

    async fn close(&mut self) -> Result<(), LinkError> {
        match std::mem::replace(&mut self.over, Over::Answered) {
            Over::Answered => Ok(()),
            Over::Open => self.steer.close().await.map_err(link_error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{CuaAction, RunMode, WindowSpace, WindowTrust};
    use docket_client::{Boxed, Events, Said, Steering, Watched};
    use docket_core::{
        CallProgress, CallRefusal, ConfirmEnd, EffectBasis, GateAnswer, WireRefusal,
    };
    use prov::ConfirmId;
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
            level: docket_core::Level::AlwaysAllow,
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
            // A transport that fails says so when the check starts; a reply that is not a
            // verdict is heard when the check is read.
            let got = match gate.check(cua_ask()).await {
                Err(error) => error,
                Ok(mut check) => check.next().await.expect_err("no verdict"),
            };
            assert_eq!(got, want, "{name}: check");
        }
    }

    #[tokio::test]
    async fn an_answered_check_has_nothing_more_to_say_and_proceed_is_a_no_op() {
        let gate = IntentsGate::over(Scripted::saying(vec![Ok(IntentsReply::Gate(
            GateAnswer::Run,
        ))]));
        let mut check = gate.check(cua_ask()).await.expect("a check");
        assert_eq!(check.proceed().await, Ok(()));
        assert_eq!(check.next().await, Ok(GateEvent::Answered(GateAnswer::Run)));
        assert!(matches!(check.next().await, Err(LinkError::Protocol(_))));
    }

    /// A transport that watches: its checks say what they were told to say and note what the
    /// run says back.
    struct Watching {
        said: Mutex<Vec<Said>>,
        steered: Arc<Mutex<Vec<&'static str>>>,
        asked: Mutex<Vec<IntentsRequest>>,
        opened: Mutex<Option<IntentsReply>>,
    }

    struct Told(VecDeque<Said>);

    impl Events for Told {
        fn next(&mut self) -> Boxed<'_, Result<Said, TransportError>> {
            let said = self.0.pop_front().ok_or(TransportError::Closed);
            Box::pin(std::future::ready(said))
        }
    }

    struct Noted(Arc<Mutex<Vec<&'static str>>>);

    impl Steering for Noted {
        fn proceed(&self) -> Boxed<'_, Result<(), TransportError>> {
            self.0.lock().expect("lock").push("proceed");
            Box::pin(std::future::ready(Ok(())))
        }

        fn close(&self) -> Boxed<'_, Result<(), TransportError>> {
            self.0.lock().expect("lock").push("close");
            Box::pin(std::future::ready(Ok(())))
        }
    }

    impl Transport for Watching {
        async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
            self.asked.lock().expect("lock").push(request);
            self.opened
                .lock()
                .expect("lock")
                .take()
                .ok_or(TransportError::Closed)
        }

        async fn watch(&self, request: IntentsRequest) -> Result<Watched, TransportError> {
            self.asked.lock().expect("lock").push(request);
            let said = self.said.lock().expect("lock").drain(..).collect();
            Ok(Watched {
                events: Box::new(Told(said)),
                steering: Arc::new(Noted(self.steered.clone())),
            })
        }
    }

    fn watching(said: Vec<Said>) -> Watching {
        Watching {
            said: Mutex::new(said),
            steered: Arc::new(Mutex::new(Vec::new())),
            asked: Mutex::new(Vec::new()),
            opened: Mutex::new(None),
        }
    }

    fn confirm_id() -> ConfirmId {
        ConfirmId::parse("c-1").unwrap()
    }

    #[tokio::test]
    async fn a_confirmation_is_told_then_proceeded_then_the_verdict_follows() {
        let gate = IntentsGate::over(watching(vec![
            Said::Progress(CallProgress::Confirming(confirm_id())),
            Said::Answer(Box::new(IntentsReply::Gate(GateAnswer::Run))),
        ]));
        let mut check = gate.check(cua_ask()).await.expect("a check");
        assert_eq!(check.next().await, Ok(GateEvent::Confirming(confirm_id())));
        assert_eq!(check.proceed().await, Ok(()));
        assert_eq!(check.next().await, Ok(GateEvent::Answered(GateAnswer::Run)));
        drop(check);
        tokio::task::yield_now().await;
        let steered = gate.transport().steered.lock().expect("lock").clone();
        assert_eq!(steered, vec!["proceed"], "an answered check is not closed");
    }

    #[tokio::test]
    async fn a_paused_run_takes_its_check_back() {
        let gate = IntentsGate::over(watching(vec![Said::Progress(CallProgress::Confirming(
            confirm_id(),
        ))]));
        let mut check = gate.check(cua_ask()).await.expect("a check");
        assert_eq!(check.next().await, Ok(GateEvent::Confirming(confirm_id())));
        assert_eq!(check.close().await, Ok(()));
        drop(check);
        tokio::task::yield_now().await;
        let steered = gate.transport().steered.lock().expect("lock").clone();
        assert_eq!(steered, vec!["close"], "closed once, not again on drop");
    }

    #[tokio::test]
    async fn a_check_let_go_of_before_its_verdict_is_withdrawn() {
        let gate = IntentsGate::over(watching(vec![Said::Progress(CallProgress::Confirming(
            confirm_id(),
        ))]));
        let check = gate.check(cua_ask()).await.expect("a check");
        drop(check);
        tokio::task::yield_now().await;
        let steered = gate.transport().steered.lock().expect("lock").clone();
        assert_eq!(steered, vec!["close"]);
    }

    #[tokio::test]
    async fn the_runs_session_is_opened_for_its_run_and_space() {
        let transport = watching(vec![]);
        *transport.opened.lock().expect("lock") =
            Some(IntentsReply::SessionOpened(docket_core::SessionOpened {
                session: SessionId::parse("s-7").unwrap(),
                task: prov::TaskId::parse("t-7").unwrap(),
            }));
        let gate = IntentsGate::over(transport);
        let run = RunId::parse("r-1").unwrap();
        let space = SpaceId::parse("work").unwrap();
        assert_eq!(
            gate.open_session(&run, &space).await,
            Ok(SessionId::parse("s-7").unwrap())
        );
        let asked = gate.transport().asked.lock().expect("lock").clone();
        assert_eq!(
            asked,
            vec![IntentsRequest::SessionOpen(SessionOpen {
                space,
                agent: AgentRef::Cua { run },
                parent: None,
                cwd: None,
                external: None,
                helper: None,
                started_from: None,
            })]
        );
    }
}
