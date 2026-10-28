//! A router gate that answers from a table.

use cua_run::{GateCheck, GateEvent, GateLink, LinkError};
use docket_core::CallRefusal;
use docket_gate_wire::{CuaAsk, GateAnswer};
use docket_wire::{GrantAnswer, GrantAsk};
use prov::{ConfirmId, RunId, SessionId, SpaceId};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// What the fake router says to the next `Gate.Check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Run it.
    Run,
    /// Refuse it.
    Refuse(CallRefusal),
    /// Ask the person (cuad hears `Confirming`, calls `Proceed`), then answer this.
    AskThen(GateAnswer),
    /// Ask the person and never answer: the check hangs after `Confirming` until it is closed
    /// (a run paused during a confirmation).
    AskAndHang,
}

/// What a check does when it has said everything it was told to say.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AfterEvents {
    /// The router is gone: the next read fails.
    Unavailable,
    /// The router has not answered yet: the next read never returns.
    Hang,
}

/// What the fake router does when asked for the per-app consent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum GrantWait {
    /// Answers at once, from the queue.
    #[default]
    Answers,
    /// The person has not answered the question: the call never returns.
    Never,
}

/// A [`GateLink`] over a queue of grant answers and a queue of verdicts; when a queue is empty
/// it grants and runs.
#[derive(Debug, Default)]
pub struct FakePdp {
    grants: Mutex<VecDeque<GrantAnswer>>,
    verdicts: Mutex<VecDeque<Verdict>>,
    checks: Mutex<Vec<CuaAsk>>,
    grant_asks: Mutex<Vec<GrantAsk>>,
    grant_wait: GrantWait,
    confirms: Mutex<u32>,
    session: Option<SessionId>,
    closes: Arc<Mutex<u32>>,
    opened: Mutex<Vec<(RunId, SpaceId)>>,
}

impl FakePdp {
    /// A gate that grants everything and runs every action.
    pub fn allow_all() -> Self {
        Self::default()
    }

    /// A gate that answers `grants` and `verdicts` in order, then grants and runs.
    pub fn scripted(grants: Vec<GrantAnswer>, verdicts: Vec<Verdict>) -> Self {
        Self {
            grants: Mutex::new(grants.into()),
            verdicts: Mutex::new(verdicts.into()),
            ..Self::default()
        }
    }

    fn confirming(&self) -> GateEvent {
        let mut confirms = self.confirms.lock().expect("fake gate lock");
        *confirms += 1;
        let id = ConfirmId::parse(&format!("c-{confirms}")).expect("a well-formed id");
        GateEvent::Confirming(id)
    }

    /// A gate whose consent question nobody answers: `Gate.Grant` never returns.
    pub fn grant_unanswered() -> Self {
        Self {
            grant_wait: GrantWait::Never,
            ..Self::default()
        }
    }

    /// Opens this session for every run (`s-1` when not set), as the router would.
    pub fn opening(mut self, session: SessionId) -> Self {
        self.session = Some(session);
        self
    }

    /// How many checks were taken back with `close`.
    pub fn closes(&self) -> u32 {
        *self.closes.lock().expect("fake gate lock")
    }

    /// The runs it opened a session for, in order, with their Space.
    pub fn opened(&self) -> Vec<(RunId, SpaceId)> {
        self.opened.lock().expect("fake gate lock").clone()
    }

    /// Every action it was asked about, in order.
    pub fn checks(&self) -> Vec<CuaAsk> {
        self.checks.lock().expect("fake gate lock").clone()
    }

    /// Every consent it was asked for.
    pub fn grant_asks(&self) -> Vec<GrantAsk> {
        self.grant_asks.lock().expect("fake gate lock").clone()
    }
}

/// One check in flight.
#[derive(Debug)]
pub struct FakeCheck {
    events: VecDeque<GateEvent>,
    after: AfterEvents,
    proceeded: Mutex<u32>,
    closed: Arc<Mutex<u32>>,
}

impl FakeCheck {
    /// How many times `close` was called.
    pub fn closed(&self) -> u32 {
        *self.closed.lock().expect("fake gate lock")
    }

    /// How many times `proceed` was called.
    pub fn proceeded(&self) -> u32 {
        *self.proceeded.lock().expect("fake gate lock")
    }
}

impl GateLink for FakePdp {
    type Check = FakeCheck;

    async fn grant(&self, ask: GrantAsk) -> Result<GrantAnswer, LinkError> {
        self.grant_asks.lock().expect("fake gate lock").push(ask);
        if self.grant_wait == GrantWait::Never {
            std::future::pending::<()>().await;
        }
        Ok(self
            .grants
            .lock()
            .expect("fake gate lock")
            .pop_front()
            .unwrap_or(GrantAnswer::Granted))
    }

    async fn open_session(
        &self,
        run: &RunId,
        space: &SpaceId,
        _conversation: &SessionId,
    ) -> Result<SessionId, LinkError> {
        self.opened
            .lock()
            .expect("fake gate lock")
            .push((run.clone(), space.clone()));
        Ok(self
            .session
            .clone()
            .unwrap_or_else(|| SessionId::parse("s-1").expect("a session id")))
    }

    async fn check(&self, ask: CuaAsk) -> Result<FakeCheck, LinkError> {
        self.checks.lock().expect("fake gate lock").push(ask);
        let verdict = self
            .verdicts
            .lock()
            .expect("fake gate lock")
            .pop_front()
            .unwrap_or(Verdict::Run);
        let (events, after) = match verdict {
            Verdict::Run => (
                vec![GateEvent::Answered(GateAnswer::Run)],
                AfterEvents::Unavailable,
            ),
            Verdict::Refuse(why) => (
                vec![GateEvent::Answered(GateAnswer::Refused(why))],
                AfterEvents::Unavailable,
            ),
            Verdict::AskThen(answer) => (
                vec![self.confirming(), GateEvent::Answered(answer)],
                AfterEvents::Unavailable,
            ),
            Verdict::AskAndHang => (vec![self.confirming()], AfterEvents::Hang),
        };
        Ok(FakeCheck {
            events: events.into(),
            after,
            proceeded: Mutex::new(0),
            closed: self.closes.clone(),
        })
    }
}

impl GateCheck for FakeCheck {
    async fn next(&mut self) -> Result<GateEvent, LinkError> {
        match self.events.pop_front() {
            Some(event) => Ok(event),
            None => match self.after {
                AfterEvents::Unavailable => Err(LinkError::Unavailable),
                AfterEvents::Hang => std::future::pending().await,
            },
        }
    }

    async fn proceed(&mut self) -> Result<(), LinkError> {
        *self.proceeded.lock().expect("fake gate lock") += 1;
        Ok(())
    }

    async fn close(&mut self) -> Result<(), LinkError> {
        *self.closed.lock().expect("fake gate lock") += 1;
        self.events.clear();
        Ok(())
    }
}
