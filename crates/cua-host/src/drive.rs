//! One run's task: the machine, its effects carried out one at a time, and the outside world
//! (the person's controls, the desktop's events, the clock) racing whatever the run is waiting
//! on.

use crate::config::CuadConfig;
use crate::events::{RunView, SupervisorEvent};
use crate::lock::locked;
use crate::settings::CuaSettings;
use cua_bus::{BudgetBody, CuaOutcome, RunEnd};
use cua_desktop::Stamp;
use cua_run::{
    Clock, Exec, Outlet, Parts, Run, RunEffect, RunInput, RunPhase, Seam, Seams, SettleWatch,
    SlotState,
};
use prov::RunId;
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;
use std::sync::{Arc, Mutex, MutexGuard, RwLock};
use std::time::Duration;
use tokio::sync::{OwnedMutexGuard, broadcast, mpsc};
use tokio::task::JoinHandle;

/// What a seam bundle needs to be driven from spawned tasks: every seam outlives the task.
pub trait Drivable: Seams {}

impl<S> Drivable for S
where
    S: Seams,
    S::Desktop: 'static,
    S::Seat: 'static,
    S::Capture: 'static,
    S::A11y: 'static,
    S::Model: 'static,
    S::Gate: 'static,
    S::Events: 'static,
    S::Time: 'static,
    <S::Capture as maskcap::WindowCapture>::Session: 'static,
    <S::Gate as cua_run::GateLink>::Check: 'static,
{
}

/// A run the supervisor keeps: how to reach its task, and what the bus shows of it.
pub(crate) struct LiveRun {
    pub control: mpsc::UnboundedSender<RunInput>,
    pub view: RunView,
    pub task: Option<JoinHandle<()>>,
}

/// What the supervisor keeps between calls.
pub(crate) struct State {
    pub next: u64,
    pub leases: cua_run::LeaseBook,
    pub live: BTreeMap<RunId, LiveRun>,
    pub pump: Option<JoinHandle<()>>,
}

/// Everything the tasks share.
pub(crate) struct Shared<S: Seams> {
    pub desktop: S::Desktop,
    pub seat: S::Seat,
    pub capture: S::Capture,
    pub a11y: S::A11y,
    pub model: Arc<tokio::sync::Mutex<S::Model>>,
    pub gate: S::Gate,
    pub events: S::Events,
    pub clock: S::Time,
    pub config: CuadConfig,
    /// What every run's model is told about the desktop when its session opens.
    pub hints: Vec<String>,
    /// The person's settings in force: the config's own until the settings file says otherwise.
    pub settings: RwLock<CuaSettings>,
    pub bus: broadcast::Sender<SupervisorEvent>,
    pub state: Mutex<State>,
}

impl<S: Seams> Shared<S> {
    pub(crate) fn of(parts: Parts<S>, config: CuadConfig, hints: Vec<String>) -> Self {
        let Parts {
            desktop,
            seat,
            capture,
            a11y,
            model,
            gate,
            events,
            clock,
        } = parts;
        Self {
            desktop,
            seat,
            capture,
            a11y,
            model: Arc::new(tokio::sync::Mutex::new(model)),
            gate,
            events,
            clock,
            settings: RwLock::new(CuaSettings::over(config.settings)),
            config,
            hints,
            bus: broadcast::channel(1024).0,
            state: Mutex::new(State {
                next: 0,
                leases: cua_run::LeaseBook::default(),
                live: BTreeMap::new(),
                pump: None,
            }),
        }
    }

    pub(crate) fn seam(&self) -> Seam<'_, S> {
        Seam {
            desktop: &self.desktop,
            seat: &self.seat,
            capture: &self.capture,
            a11y: &self.a11y,
            gate: &self.gate,
            events: &self.events,
            clock: &self.clock,
        }
    }

    /// The supervisor's state. A task that panicked while holding the lock left the state as
    /// consistent as every update keeps it, so a poisoned lock is taken as it is rather than
    /// panicking every later call.
    pub(crate) fn state(&self) -> MutexGuard<'_, State> {
        locked(&self.state)
    }

    /// Tells whoever listens; nobody listening is not an error.
    pub(crate) fn tell(&self, event: SupervisorEvent) {
        let _ = self.bus.send(event);
    }

    /// The bus's view of `run` after a step, and whether the window's input is stopped.
    fn update(&self, run: &Run) {
        let mut state = self.state();
        if let Some(live) = state.live.get_mut(&run.id) {
            live.view.phase = run.phase.clone();
            live.view.budget = BudgetBody {
                limits: run.task.budget,
                used: run.used,
            };
            if let Some(obs) = &run.working {
                live.view.title = obs.window.title.value.clone();
            }
        }
        let slot = match run.phase {
            RunPhase::Paused(_) | RunPhase::TakenOver(..) | RunPhase::Confirming(..) => {
                SlotState::Suspended
            }
            _ => SlotState::Held,
        };
        state.leases = state.leases.set_state(&run.id, slot);
    }
}

/// What an effect's answers look like on the way back to the task.
enum Wire {
    Input(RunInput),
    Signal(cua_run::CuaSignal),
}

struct Channel(mpsc::UnboundedSender<Wire>);

impl Outlet for Channel {
    fn input(&mut self, input: RunInput) {
        let _ = self.0.send(Wire::Input(input));
    }

    fn signal(&mut self, signal: cua_run::CuaSignal) {
        let _ = self.0.send(Wire::Signal(signal));
    }
}

/// An effect that starts work against the world as it was: when the person interrupts, it is
/// dropped rather than run. The rest (records, signals, reports, lease handling) still happen.
fn is_work(effect: &RunEffect) -> bool {
    matches!(
        effect,
        RunEffect::Grant { .. }
            | RunEffect::AcquireLease(_)
            | RunEffect::Capture { .. }
            | RunEffect::Snapshot(_)
            | RunEffect::FetchContext
            | RunEffect::CallModel { .. }
            | RunEffect::Check(_)
            | RunEffect::Proceed
            | RunEffect::Inject(_)
            | RunEffect::VerifyTree(_)
    )
}

/// Whether the screen's damage was asked for and ran dry (nothing more will come until the
/// next action).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Damage {
    Flowing,
    Dry,
}

/// The run and the effects it has not carried out yet.
struct Machine {
    run: Run,
    queue: VecDeque<RunEffect>,
}

impl Machine {
    fn run(&self) -> &Run {
        &self.run
    }

    fn feed<S: Seams>(&mut self, shared: &Shared<S>, input: RunInput, at: Stamp) {
        let effects = self.run.advance(input, at);
        shared.update(&self.run);
        self.queue.extend(effects);
    }

    /// The person or the desktop interrupts: what was about to start is dropped, the input is the
    /// machine's to answer.
    fn interrupt<S: Seams>(&mut self, shared: &Shared<S>, input: RunInput, at: Stamp) {
        if !self.run().phase.is_finished() {
            self.queue.retain(|e| !is_work(e));
        }
        self.feed(shared, input, at);
    }
}

fn run_end<S: Seams>(exec: &Exec<'_, S>, run: &Run, outcome: CuaOutcome) -> RunEnd {
    RunEnd {
        outcome,
        steps: run.used.steps,
        active: run.used.active,
        cost: run.used.spend,
        report: exec.report_delivery(),
    }
}

/// Lets go of a run's window and its place in the list when its task ends, however it ends (a
/// task that panics must not leave its window held for good).
struct Cleanup<S: Seams> {
    shared: Arc<Shared<S>>,
    run: RunId,
}

impl<S: Seams> Drop for Cleanup<S> {
    fn drop(&mut self) {
        let mut state = self.shared.state();
        state.leases = state.leases.release(&self.run);
        state.live.remove(&self.run);
    }
}

/// Runs one run to its end: the machine's effects one at a time, each raced against the
/// controls, the desktop's events and the clock; then the bookkeeping of its end.
pub(crate) async fn drive<S: Drivable>(
    shared: Arc<Shared<S>>,
    run: Run,
    initial: Vec<RunEffect>,
    mut control: mpsc::UnboundedReceiver<RunInput>,
) {
    let id = run.id.clone();
    let cleanup = Cleanup {
        shared: shared.clone(),
        run: id.clone(),
    };
    let mut exec = Exec::<S>::new(shared.seam(), id.clone(), run.task.clone())
        .with_hints(shared.hints.clone());
    let (tx, mut wire) = mpsc::unbounded_channel::<Wire>();
    let mut outlet = Channel(tx);
    let mut guard: Option<OwnedMutexGuard<S::Model>> = None;
    let mut machine = Machine {
        run,
        queue: initial.into(),
    };
    let mut damage = Damage::Flowing;
    let outcome = loop {
        while let Ok(input) = control.try_recv() {
            machine.interrupt(&shared, input, shared.clock.now());
        }
        while let Ok(event) = wire.try_recv() {
            heard(&shared, &id, &mut machine, event);
        }
        if let Some(effect) = machine.queue.pop_front() {
            let deadline = cua_run::deadline_of(&effect);
            if Exec::<S>::needs_model(&effect) && guard.is_none() {
                // Waiting for the model is not the model call: no deadline, but the controls and
                // the budget still reach the run.
                let lock = shared.model.clone().lock_owned();
                match race(
                    &shared,
                    &id,
                    &mut machine,
                    &mut control,
                    &mut wire,
                    lock,
                    None,
                )
                .await
                {
                    Raced::Finished(held) => guard = Some(held),
                    Raced::Interrupted | Raced::Ended | Raced::TimedOut => continue,
                }
            }
            let limit = deadline.as_ref().map(|d| d.after);
            let work = exec.carry_out(effect, guard.as_deref_mut(), &mut outlet);
            if let Raced::TimedOut = race(
                &shared,
                &id,
                &mut machine,
                &mut control,
                &mut wire,
                work,
                limit,
            )
            .await
                && let Some(failure) = deadline.and_then(|d| d.failure)
            {
                machine.feed(&shared, failure, shared.clock.now());
            }
            continue;
        }
        if let Some(outcome) = machine.run().phase.outcome() {
            break outcome;
        }
        // A run that waits on the person lets the model go: it holds no model while paused, and
        // opens a new model session when it goes on.
        if guard.is_some()
            && matches!(
                machine.run().phase,
                RunPhase::Paused(_) | RunPhase::TakenOver(..) | RunPhase::WaitingForUser(..)
            )
        {
            guard = None;
            exec.forget_model_session();
        }
        // Nothing to do: wait for the person, the desktop, the screen or the clock.
        let wake = machine.run().wake();
        let watching = matches!(machine.run().track.settle, SettleWatch::Damage { .. })
            && damage == Damage::Flowing;
        let nap = wake.map_or(Duration::MAX, |at| {
            Duration::from_millis(at.0.saturating_sub(shared.clock.now().0))
        });
        tokio::select! {
            biased;
            Some(input) = control.recv() => machine.interrupt(&shared, input, shared.clock.now()),
            Some(event) = wire.recv() => heard(&shared, &id, &mut machine, event),
            seen = exec.next_damage(), if watching => match seen {
                Some(change) => machine.feed(&shared, RunInput::Damage(change), shared.clock.now()),
                None => damage = Damage::Dry,
            },
            () = tokio::time::sleep(nap), if wake.is_some() => {
                let now = shared.clock.now();
                machine.feed(&shared, RunInput::Tick, wake.map_or(now, |due| now.max(due)));
            }
        }
        if !matches!(machine.run().track.settle, SettleWatch::Damage { .. }) {
            damage = Damage::Flowing;
        }
    };
    while let Ok(event) = wire.try_recv() {
        heard(&shared, &id, &mut machine, event);
    }
    let end = run_end(&exec, machine.run(), outcome);
    drop(guard);
    drop(exec);
    drop(cleanup);
    shared.tell(SupervisorEvent::Ended(id.clone(), Box::new(end)));
    shared.tell(SupervisorEvent::Removed(id));
}

/// How a race between an effect and the outside world ended.
enum Raced<T> {
    /// The effect finished.
    Finished(T),
    /// The person or the desktop interrupted it; the machine has been told.
    Interrupted,
    /// The clock ended the run (the active budget); the machine has been told.
    Ended,
    /// The effect passed its deadline.
    TimedOut,
}

/// Races `work` against the controls, the effects' answers, the effect's `limit` and the
/// machine's own clock: the active budget ticks while an effect runs, so a stalled neighbour
/// cannot hold a run past its budget.
async fn race<S: Seams, T>(
    shared: &Shared<S>,
    id: &RunId,
    machine: &mut Machine,
    control: &mut mpsc::UnboundedReceiver<RunInput>,
    wire: &mut mpsc::UnboundedReceiver<Wire>,
    work: impl Future<Output = T>,
    limit: Option<Duration>,
) -> Raced<T> {
    tokio::pin!(work);
    let timer = tokio::time::sleep(limit.unwrap_or(Duration::ZERO));
    tokio::pin!(timer);
    loop {
        let wake = machine.run().wake();
        let nap = wake.map_or(Duration::ZERO, |at| {
            Duration::from_millis(at.0.saturating_sub(shared.clock.now().0))
        });
        tokio::select! {
            biased;
            Some(input) = control.recv() => {
                machine.interrupt(shared, input, shared.clock.now());
                return Raced::Interrupted;
            }
            Some(event) = wire.recv() => heard(shared, id, machine, event),
            done = &mut work => return Raced::Finished(done),
            () = &mut timer, if limit.is_some() => return Raced::TimedOut,
            () = tokio::time::sleep(nap), if wake.is_some() => {
                let now = shared.clock.now();
                machine.feed(shared, RunInput::Tick, wake.map_or(now, |due| now.max(due)));
                if machine.run().phase.is_finished() {
                    machine.queue.retain(|e| !is_work(e));
                    return Raced::Ended;
                }
            }
        }
    }
}

/// What an effect said as it ran: an input for the machine, or a signal for the bus.
fn heard<S: Seams>(shared: &Shared<S>, id: &RunId, machine: &mut Machine, event: Wire) {
    match event {
        Wire::Input(input) => machine.feed(shared, input, shared.clock.now()),
        Wire::Signal(signal) => shared.tell(SupervisorEvent::Signal(id.clone(), signal)),
    }
}
