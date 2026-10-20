//! Carrying out the machine's effects against the seams.
//!
//! The machine decides; [`Exec`] does: it holds the lease, the capture session and the check in
//! flight of one run, and turns each [`RunEffect`] into calls on the seams and the
//! [`RunInput`]s they answer with. It is runtime-free (it only awaits seam futures), so the
//! harness in `cua-fake` and the supervisor in `cuad` drive the very same code; what differs
//! is how they wait (the supervisor races an effect against the person's controls and the
//! clock, the harness has none of either).

mod cost;
mod deadline;
mod fit;
mod front;
mod observe;
mod phase;
mod session;

use crate::machine::effect::{CuaSignal, RunEffect};
use crate::machine::input::RunInput;
use crate::model::CuaModel;
use crate::screen::ScreenRead;
use crate::seams::{
    Clock, GateCheck, GateEvent, GateLink, LinkError, MessageLink, Parts, RecordSink, Seams,
};
use cua_bus::{CuaRecord, CuaTask, ReportDelivery};
pub use deadline::{Deadline, deadline_of};
use docket_core::{Delivery, GrantAsk, SendRefusal, WireRefusal};
use maskcap::DamageAt;
pub use phase::{Progress, Step, StepNote};
use porter_core::Count;
use prov::{MessageId, RunId};
use session::Held;

/// Where an effect's answers go: the inputs of the machine, and the signals of the bus.
pub trait Outlet: Send {
    /// An input for the machine, in the order it happened.
    fn input(&mut self, input: RunInput);

    /// A signal for the bus.
    fn signal(&mut self, signal: CuaSignal);

    /// A line for the daemon's log about taking a window. Nothing by default.
    fn note(&mut self, note: StepNote) {
        let _ = note;
    }
}

/// Inputs and signals kept in order, for a driver that has nothing to race.
#[derive(Debug, Default)]
pub struct Collected {
    /// The inputs.
    pub inputs: std::collections::VecDeque<RunInput>,
    /// The signals.
    pub signals: Vec<CuaSignal>,
    /// The log lines of taking a window, in order.
    pub notes: Vec<StepNote>,
}

impl Outlet for Collected {
    fn input(&mut self, input: RunInput) {
        self.inputs.push_back(input);
    }

    fn signal(&mut self, signal: CuaSignal) {
        self.signals.push(signal);
    }

    fn note(&mut self, note: StepNote) {
        self.notes.push(note);
    }
}

/// The seams of a run, by reference: everything of [`Parts`] but the model, which a run holds
/// by `&mut` for as long as it lasts.
#[derive(Debug)]
pub struct Seam<'a, S: Seams> {
    /// The desktop.
    pub desktop: &'a S::Desktop,
    /// The seat.
    pub seat: &'a S::Seat,
    /// Capture.
    pub capture: &'a S::Capture,
    /// The accessibility source.
    pub a11y: &'a S::A11y,
    /// The gate.
    pub gate: &'a S::Gate,
    /// The event log and the message path.
    pub events: &'a S::Events,
    /// The clock.
    pub clock: &'a S::Time,
}

impl<'a, S: Seams> Clone for Seam<'a, S> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, S: Seams> Copy for Seam<'a, S> {}

impl<'a, S: Seams> Seam<'a, S> {
    /// The seams of `parts`.
    pub fn of(parts: &'a Parts<S>) -> Self {
        Self {
            desktop: &parts.desktop,
            seat: &parts.seat,
            capture: &parts.capture,
            a11y: &parts.a11y,
            gate: &parts.gate,
            events: &parts.events,
            clock: &parts.clock,
        }
    }
}

/// Whether the model's session has been opened (`CuaModel::begin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelSession {
    NotYet,
    Open,
}

/// Whether the record sink has been told of the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opened {
    No,
    Yes,
}

/// How the last report ended, for the record of the end.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reported {
    Nothing,
    Delivered(Delivery),
    Refused(SendRefusal),
}

/// Carries out one run's effects.
pub struct Exec<'a, S: Seams> {
    seam: Seam<'a, S>,
    run: RunId,
    task: CuaTask,
    held: Option<Held<S>>,
    staged: observe::Staged,
    check: Option<<S::Gate as GateLink>::Check>,
    notes: Vec<porter_infer::StepNote>,
    last_message: Option<MessageId>,
    model: ModelSession,
    announced: Option<porter_infer::ServedBy>,
    reported: Reported,
    opened: Opened,
    hints: Vec<String>,
    seen: Option<fit::Seen>,
    record_failures: Count,
}

impl<S: Seams> std::fmt::Debug for Exec<'_, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Exec")
            .field("run", &self.run)
            .finish_non_exhaustive()
    }
}

impl<'a, S: Seams> Exec<'a, S> {
    /// An executor for `run` doing `task`.
    pub fn new(seam: Seam<'a, S>, run: RunId, task: CuaTask) -> Self {
        Self {
            seam,
            run,
            task,
            held: None,
            staged: observe::Staged::default(),
            check: None,
            notes: Vec::new(),
            last_message: None,
            model: ModelSession::NotYet,
            announced: None,
            reported: Reported::Nothing,
            opened: Opened::No,
            hints: Vec::new(),
            seen: None,
            record_failures: Count(0),
        }
    }

    /// The same executor telling the model these short hints about the desktop when its session
    /// opens (the shortcut line of [`crate::shortcut_hint`], for one). The caller builds them
    /// once from what it knows of the desktop; nothing here reads an environment.
    pub fn with_hints(self, hints: Vec<String>) -> Self {
        Self { hints, ..self }
    }

    /// Whether `effect` talks to the model, and so needs it handed in.
    pub fn needs_model(effect: &RunEffect) -> bool {
        matches!(
            effect,
            RunEffect::CallModel { .. } | RunEffect::ReadScreen(_)
        )
    }

    /// The next change of the screen, from the capture session; `None` when it has none to
    /// give (no session, or the source went away).
    pub async fn next_damage(&mut self) -> Option<DamageAt> {
        use maskcap::CaptureSession;
        match self.held.as_mut() {
            Some(held) => held.session().damage().await.ok(),
            None => None,
        }
    }

    /// Carries out one effect, pushing what it answers with into `out`. `model` is the run's
    /// model, when the caller has it; a model call without one fails fatally.
    pub async fn carry_out(
        &mut self,
        effect: RunEffect,
        model: Option<&mut S::Model>,
        out: &mut impl Outlet,
    ) {
        if self.opened == Opened::No {
            self.opened = Opened::Yes;
            self.seam.events.open(&self.run, &self.task).await;
        }
        match effect {
            RunEffect::Grant { app, space, level } => {
                let answer = self.seam.gate.grant(GrantAsk { app, space, level }).await;
                out.input(match answer {
                    Ok(answer) => RunInput::Granted(answer),
                    Err(error) => RunInput::GrantFailed(error),
                });
            }
            RunEffect::AcquireLease(request) => self.acquire(request, out).await,
            RunEffect::ReleaseLease(lease) => self.release(lease).await,
            RunEffect::SuspendLease(lease, why) => self.suspend(lease, why).await,
            RunEffect::ResumeLease(lease) => self.resume(lease).await,
            RunEffect::Snapshot(budget) => self.snapshot(budget).await,
            RunEffect::FetchContext => self.staged.no_context(),
            RunEffect::Capture { want, hints } => self.capture(want, hints, out).await,
            RunEffect::CallModel { request, frame } => match model {
                Some(model) => self.call_model(model, *request, frame, out).await,
                None => out.input(RunInput::ModelFailed(crate::model::CuaModelError::Fatal)),
            },
            RunEffect::ReadScreen(text) => {
                let read = match model {
                    Some(model) => model.classify_screen(&text).await,
                    None => ScreenRead::Unavailable,
                };
                out.input(RunInput::ScreenRead(read));
            }
            RunEffect::CancelModel | RunEffect::CancelSettle | RunEffect::WatchSettle { .. } => {}
            RunEffect::Check(ask) => self.check(*ask, out).await,
            RunEffect::Proceed => self.proceed(out).await,
            RunEffect::Inject(planned) => self.inject(planned, out).await,
            RunEffect::VerifyTree(selector) => self.verify(selector, out).await,
            RunEffect::Glow(glow) => self.glow(glow).await,
            RunEffect::Emit(signal) => out.signal(signal),
            RunEffect::Record(record) => self.record(*record, out).await,
            RunEffect::Report(draft) => self.report(*draft).await,
        }
    }

    async fn check(&mut self, ask: docket_core::CuaAsk, out: &mut impl Outlet) {
        match self.seam.gate.check(ask).await {
            Ok(check) => {
                self.check = Some(check);
                self.await_verdict(out).await;
            }
            Err(error) => out.input(RunInput::GateFailed(error)),
        }
    }

    async fn proceed(&mut self, out: &mut impl Outlet) {
        let Some(check) = self.check.as_mut() else {
            return;
        };
        match check.proceed().await {
            Ok(()) => self.await_verdict(out).await,
            Err(error) => out.input(RunInput::GateFailed(error)),
        }
    }

    /// Reads what the router says next: that it will ask the person (and waits for `Proceed`),
    /// or its answer.
    async fn await_verdict(&mut self, out: &mut impl Outlet) {
        let Some(check) = self.check.as_mut() else {
            return;
        };
        match check.next().await {
            Ok(GateEvent::Confirming(id)) => out.input(RunInput::Confirming(id)),
            Ok(GateEvent::Answered(answer)) => {
                self.check = None;
                out.input(RunInput::Verdict(answer));
            }
            Err(error) => {
                self.check = None;
                out.input(RunInput::GateFailed(error));
            }
        }
    }

    /// How many records the log refused so far. A run that cannot write the record of an
    /// injected action ends (`FailReason::Log`); the others are only counted here, for the
    /// caller to surface.
    pub fn record_failures(&self) -> Count {
        self.record_failures
    }

    async fn record(&mut self, mut record: CuaRecord, out: &mut impl Outlet) {
        if let CuaRecord::Finished { report, .. } = &mut record
            && let Reported::Delivered(delivery) = &self.reported
        {
            *report = Some(delivery.message.clone());
        }
        // A record of input that went out must be written: with nothing to account for it the
        // run stops. Any other record that cannot be written is counted and the run goes on
        // (memoryd may be away); nothing here retries it.
        let injected = matches!(&record, CuaRecord::Step { input: Some(_), .. });
        if self.seam.events.record(record).await.is_err() {
            self.record_failures = Count(self.record_failures.0.saturating_add(1));
            if injected {
                out.input(RunInput::RecordFailed);
            }
        }
    }

    async fn report(&mut self, draft: docket_core::MessageDraft) {
        let sent = self
            .seam
            .events
            .send(self.task.session.clone(), draft)
            .await;
        self.reported = match sent {
            Ok(delivery) => Reported::Delivered(delivery),
            Err(LinkError::Refused(WireRefusal::Send(why))) => Reported::Refused(why),
            Err(_) => Reported::Nothing,
        };
    }

    /// What became of the last report: the message that carried it, or why none did (a router
    /// that could not be reached counts as nobody to receive it).
    pub fn report_delivery(&self) -> ReportDelivery {
        match &self.reported {
            Reported::Delivered(delivery) => ReportDelivery::Sent(delivery.clone()),
            Reported::Refused(why) => ReportDelivery::Refused(*why),
            Reported::Nothing => ReportDelivery::Refused(SendRefusal::NoRecipient),
        }
    }

    /// The clock the run is on.
    pub fn now(&self) -> cua_desktop::Stamp {
        self.seam.clock.now()
    }
}
