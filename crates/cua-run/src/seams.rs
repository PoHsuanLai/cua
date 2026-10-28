//! What cuad plugs into a run: the clock, the router's gate, the event log and the message
//! path. Closed sets of implementations are generic parameters, never `dyn`.

use crate::model::CuaModel;
use crate::rules::Rules;
use a11y_tree::A11ySource;
use cua_bus::{CuaRecord, CuaTask};
use cua_desktop::{Desktop, LeaseControl, Stamp};
use docket_core::{Delivery, InboundLine, InboxAsk, MessageDraft};
use docket_gate_wire::{CuaAsk, GateAnswer};
use docket_wire::{GrantAnswer, GrantAsk, WireRefusal};
use maskcap::WindowCapture;
use porter_ids::UnixSeconds;
use prov::{ConfirmId, RunId, SessionId, SpaceId};
use seat_input::AgentSeat;
use serde::{Deserialize, Serialize};
use std::future::Future;

/// Why a call to a neighbour failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum LinkError {
    /// The neighbour is not there (the router, memoryd).
    #[error("unavailable")]
    Unavailable,
    /// It refused the request outright.
    #[error("refused: {0:?}")]
    Refused(WireRefusal),
    /// Its answer did not follow the protocol.
    #[error("protocol: {0}")]
    Protocol(String),
}

/// The clock. Only the daemon reads the system's.
pub trait Clock: Send + Sync {
    /// Now, on cuad's clock.
    fn now(&self) -> Stamp;

    /// Now, as wall time.
    fn unix(&self) -> UnixSeconds;

    /// Ready once `limit` has passed. A clock with no timers (a harness) never is, so what is
    /// bounded by it waits as long as it likes.
    fn after(&self, limit: std::time::Duration) -> impl Future<Output = ()> + Send {
        let _ = limit;
        std::future::pending()
    }
}

/// What a check in flight reports.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GateEvent {
    /// The router will ask the person; cuad must stop its input, then call `proceed`.
    Confirming(ConfirmId),
    /// The router's answer. The last event.
    Answered(GateAnswer),
}

/// One `Gate.Check` in flight: the Request object the router returned.
pub trait GateCheck: Send {
    /// The next thing the router says.
    fn next(&mut self) -> impl Future<Output = Result<GateEvent, LinkError>> + Send;

    /// `Request.Proceed`: the lease is suspended, show the confirmation now.
    fn proceed(&mut self) -> impl Future<Output = Result<(), LinkError>> + Send;

    /// `Request.Close`: takes the check back (the run is paused): a confirmation that is up
    /// comes down and no answer follows.
    fn close(&mut self) -> impl Future<Output = Result<(), LinkError>> + Send;
}

/// `Intents1.Gate`: the one decision point for every pixel step.
pub trait GateLink: Send + Sync {
    /// The check in flight.
    type Check: GateCheck;

    /// The per-app consent (`Gate.Grant`): the router asks the person on first use.
    fn grant(&self, ask: GrantAsk) -> impl Future<Output = Result<GrantAnswer, LinkError>> + Send;

    /// Opens the session a run acts in (`Session.Open` with `AgentRef::Cua { run }`). The
    /// router finds a run's rules by that session, so it comes before the run's first check.
    /// `conversation` is the session that started the run: the run follows its mode.
    fn open_session(
        &self,
        run: &RunId,
        space: &SpaceId,
        conversation: &SessionId,
    ) -> impl Future<Output = Result<SessionId, LinkError>> + Send;

    /// One step's check (`Gate.Check`).
    fn check(&self, ask: CuaAsk) -> impl Future<Output = Result<Self::Check, LinkError>> + Send;
}

/// Where run records go: almanac's event log, through memoryd.
pub trait RecordSink: Send + Sync {
    /// Appends one record of a run.
    fn record(&self, record: CuaRecord) -> impl Future<Output = Result<(), LinkError>> + Send;

    /// A run is about to begin: what a sink needs to know about it besides each record (the
    /// session it acts for, its Space and app), which a run that is blocked before it has a
    /// window never says in a record of its own. Nothing to do by default.
    fn open(&self, run: &RunId, task: &CuaTask) -> impl Future<Output = ()> + Send {
        let _ = (run, task);
        async {}
    }
}

/// The message path: a run's reports out, the person's and the planner's messages in. A message
/// carries no authority: whatever arrives is input to the run, and nothing in it widens what the
/// run may do.
pub trait MessageLink: Send + Sync {
    /// Sends a report (`Intents1.Message.Send`); the router stamps the id, sender and label.
    fn send(
        &self,
        session: SessionId,
        draft: MessageDraft,
    ) -> impl Future<Output = Result<Delivery, LinkError>> + Send;

    /// The messages that wait for this run (`Intents1.Message.Inbox`).
    fn inbox(
        &self,
        ask: InboxAsk,
    ) -> impl Future<Output = Result<Vec<InboundLine>, LinkError>> + Send;
}

/// Everything a run is carried out against, as types: the real set in cua-system (`SystemSeams`), the
/// fakes in `cua-fake` (`FakeSeams`). Closed sets of implementations are generic parameters
/// bundled here, never `dyn`.
pub trait Seams: Send + Sync + 'static {
    /// What is on the desktop, and leasing it.
    type Desktop: Desktop + LeaseControl;
    /// The agent's hands.
    type Seat: AgentSeat;
    /// Masked capture.
    type Capture: WindowCapture;
    /// Accessibility snapshots.
    type A11y: A11ySource;
    /// The model.
    type Model: CuaModel;
    /// The router's gate.
    type Gate: GateLink;
    /// The event log and the message path.
    type Events: RecordSink + MessageLink;
    /// The clock.
    type Time: Clock;
}

/// The values of a [`Seams`] bundle.
pub struct Parts<S: Seams> {
    /// The desktop.
    pub desktop: S::Desktop,
    /// The seat.
    pub seat: S::Seat,
    /// Capture.
    pub capture: S::Capture,
    /// The accessibility source.
    pub a11y: S::A11y,
    /// The model.
    pub model: S::Model,
    /// The gate.
    pub gate: S::Gate,
    /// The event log and the message path.
    pub events: S::Events,
    /// The clock.
    pub clock: S::Time,
    /// The screen rules, parsed by the host from the shipped file.
    pub rules: Rules,
}

impl<S: Seams> std::fmt::Debug for Parts<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Parts").finish_non_exhaustive()
    }
}
