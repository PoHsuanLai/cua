//! A router gate that answers from a table.

use cua_run::{GateCheck, GateEvent, GateLink, LinkError};
use docket_core::{CallRefusal, CuaAsk, GateAnswer, GrantAnswer, GrantAsk};
use prov::ConfirmId;
use std::collections::VecDeque;
use std::sync::Mutex;

/// What the fake router says to the next `Gate.Check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Run it.
    Run,
    /// Refuse it.
    Refuse(CallRefusal),
    /// Ask the person (cuad hears `Confirming`, calls `Proceed`), then answer this.
    AskThen(GateAnswer),
}

/// A [`GateLink`] over a queue of grant answers and a queue of verdicts; when a queue is empty
/// it grants and runs.
#[derive(Debug, Default)]
pub struct FakePdp {
    grants: Mutex<VecDeque<GrantAnswer>>,
    verdicts: Mutex<VecDeque<Verdict>>,
    checks: Mutex<Vec<CuaAsk>>,
    grant_asks: Mutex<Vec<GrantAsk>>,
    confirms: Mutex<u32>,
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
    proceeded: Mutex<u32>,
}

impl FakeCheck {
    /// How many times `proceed` was called.
    pub fn proceeded(&self) -> u32 {
        *self.proceeded.lock().expect("fake gate lock")
    }
}

impl GateLink for FakePdp {
    type Check = FakeCheck;

    async fn grant(&self, ask: GrantAsk) -> Result<GrantAnswer, LinkError> {
        self.grant_asks.lock().expect("fake gate lock").push(ask);
        Ok(self
            .grants
            .lock()
            .expect("fake gate lock")
            .pop_front()
            .unwrap_or(GrantAnswer::Granted))
    }

    async fn check(&self, ask: CuaAsk) -> Result<FakeCheck, LinkError> {
        self.checks.lock().expect("fake gate lock").push(ask);
        let verdict = self
            .verdicts
            .lock()
            .expect("fake gate lock")
            .pop_front()
            .unwrap_or(Verdict::Run);
        let events = match verdict {
            Verdict::Run => vec![GateEvent::Answered(GateAnswer::Run)],
            Verdict::Refuse(why) => vec![GateEvent::Answered(GateAnswer::Refused(why))],
            Verdict::AskThen(answer) => {
                let mut confirms = self.confirms.lock().expect("fake gate lock");
                *confirms += 1;
                let id = ConfirmId::parse(&format!("c-{confirms}")).expect("a well-formed id");
                vec![GateEvent::Confirming(id), GateEvent::Answered(answer)]
            }
        };
        Ok(FakeCheck {
            events: events.into(),
            proceeded: Mutex::new(0),
        })
    }
}

impl GateCheck for FakeCheck {
    async fn next(&mut self) -> Result<GateEvent, LinkError> {
        self.events.pop_front().ok_or(LinkError::Unavailable)
    }

    async fn proceed(&mut self) -> Result<(), LinkError> {
        *self.proceeded.lock().expect("fake gate lock") += 1;
        Ok(())
    }
}
