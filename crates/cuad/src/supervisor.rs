//! The supervisor: every live run, the lease book, and the control calls on them.

use crate::config::CuadConfig;
use crate::drive::{Drivable, LiveRun, Shared, drive};
use crate::events::{RunView, SupervisorEvent};
use crate::guard::guard;
use cua_bus::{
    BudgetBody, ControlRefusal, CuaCaller, CuaMember, CuaTask, ModeSupport, QuestionId, TaskTarget,
};
use cua_desktop::Desktop;
use cua_run::{Clock, LeaseBook, Parts, Run, RunEnv, RunInput, RunPhase, Seams, mode_support};
use prov::{Labelled, RunId};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// The id of the `n`th run cuad starts: `r-<n>`.
pub fn mint_run_id(n: u64) -> RunId {
    RunId::parse(&format!("r-{n}")).expect("`r-` and digits are an id")
}

/// Owns the runs. Each run is one task over one `Run` machine; the supervisor carries out its
/// effects against the seams and feeds the answers back.
///
/// The model is one seam for all runs and holds one session, so a run keeps it for as long as it
/// lasts: runs on different windows work side by side, but their model turns take turns, one
/// run at a time (FINDINGS, upstream asks).
pub struct Supervisor<S: Seams> {
    shared: Arc<Shared<S>>,
}

impl<S: Seams> std::fmt::Debug for Supervisor<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor").finish_non_exhaustive()
    }
}

impl<S: Seams> Drop for Supervisor<S> {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().expect("supervisor lock");
        if let Some(pump) = state.pump.take() {
            pump.abort();
        }
        for live in state.live.values_mut() {
            if let Some(task) = live.task.take() {
                task.abort();
            }
        }
    }
}

/// Whether a control call makes sense in a phase: the shell's buttons only show what applies,
/// but a call can arrive late.
fn applies(member: CuaMember, phase: &RunPhase) -> bool {
    let live = !phase.is_finished();
    match member {
        CuaMember::Cancel => live,
        CuaMember::Pause => matches!(
            phase,
            RunPhase::Observing(_)
                | RunPhase::Thinking(_)
                | RunPhase::Authorising(_)
                | RunPhase::Confirming(..)
                | RunPhase::Acting(_)
                | RunPhase::Settling(..)
                | RunPhase::Verifying(_)
                | RunPhase::WaitingForUser(..)
        ),
        CuaMember::Resume => matches!(phase, RunPhase::Paused(_)),
        CuaMember::TakeOver => {
            live && !matches!(
                phase,
                RunPhase::Granting | RunPhase::Leasing | RunPhase::TakenOver(..)
            )
        }
        CuaMember::HandBack => matches!(phase, RunPhase::TakenOver(..)),
        CuaMember::Start | CuaMember::Runs | CuaMember::StopAll | CuaMember::Answer => false,
    }
}

/// The input a control member is.
fn input_of(member: CuaMember) -> Option<RunInput> {
    match member {
        CuaMember::Cancel => Some(RunInput::UserCancel),
        CuaMember::Pause => Some(RunInput::UserPause),
        CuaMember::Resume => Some(RunInput::UserResume),
        CuaMember::TakeOver => Some(RunInput::UserTakeOver),
        CuaMember::HandBack => Some(RunInput::UserHandBack),
        CuaMember::Start | CuaMember::Runs | CuaMember::StopAll | CuaMember::Answer => None,
    }
}

impl<S: Seams> Supervisor<S> {
    /// A supervisor over `parts` under `config`.
    pub fn new(parts: Parts<S>, config: CuadConfig) -> Self {
        Self {
            shared: Arc::new(Shared::of(parts, config)),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &CuadConfig {
        &self.shared.config
    }

    /// The desktop.
    pub fn desktop(&self) -> &S::Desktop {
        &self.shared.desktop
    }

    /// The seat.
    pub fn seat(&self) -> &S::Seat {
        &self.shared.seat
    }

    /// Capture.
    pub fn capture(&self) -> &S::Capture {
        &self.shared.capture
    }

    /// The accessibility source.
    pub fn a11y(&self) -> &S::A11y {
        &self.shared.a11y
    }

    /// The router's gate.
    pub fn gate(&self) -> &S::Gate {
        &self.shared.gate
    }

    /// The event log and the message path.
    pub fn events(&self) -> &S::Events {
        &self.shared.events
    }

    /// The clock.
    pub fn clock(&self) -> &S::Time {
        &self.shared.clock
    }

    /// Which run holds which window now.
    pub fn lease_book(&self) -> LeaseBook {
        self.shared
            .state
            .lock()
            .expect("supervisor lock")
            .leases
            .clone()
    }

    /// How many runs have been started (the next id is `r-<this + 1>`).
    pub fn started(&self) -> u64 {
        self.shared.state.lock().expect("supervisor lock").next
    }

    /// The ids of the runs now.
    pub fn runs(&self) -> Vec<RunId> {
        self.shared
            .state
            .lock()
            .expect("supervisor lock")
            .live
            .keys()
            .cloned()
            .collect()
    }

    /// What the bus shows of one live run.
    pub fn view(&self, run: &RunId) -> Option<RunView> {
        self.shared
            .state
            .lock()
            .expect("supervisor lock")
            .live
            .get(run)
            .map(|live| live.view.clone())
    }

    /// Everything that happens from now on, in order. A listener that falls too far behind
    /// misses the oldest events and is told so.
    pub fn subscribe(&self) -> broadcast::Receiver<SupervisorEvent> {
        self.shared.bus.subscribe()
    }
}

impl<S: Drivable> Supervisor<S> {
    /// Starts a run for `task` (`guard` first; one run per window; the mode must be supported).
    pub async fn start(&self, caller: CuaCaller, task: CuaTask) -> Result<RunId, ControlRefusal> {
        guard(
            caller,
            CuaMember::Start,
            self.shared.config.settings.enabled,
        )?;
        let caps = self.shared.desktop.caps();
        if let ModeSupport::Unsupported(_) = mode_support(&caps, task.mode) {
            return Err(ControlRefusal::ModeUnsupported);
        }
        if let TaskTarget::Window(ident) = &task.target {
            let known = self
                .shared
                .desktop
                .windows()
                .await
                .map_err(|_| ControlRefusal::WindowUnknown)?;
            if !known.iter().any(|w| &w.ident == ident) {
                return Err(ControlRefusal::WindowUnknown);
            }
        }
        let env = RunEnv::new(&self.shared.config.settings, caps);
        let (control, inbox) = mpsc::unbounded_channel();
        let (run, effects) = {
            let mut state = self.shared.state.lock().expect("supervisor lock");
            let id = mint_run_id(state.next + 1);
            if let TaskTarget::Window(ident) = &task.target {
                state.leases = state
                    .leases
                    .claim(ident, &id)
                    .map_err(|_| ControlRefusal::LeaseHeld)?;
            }
            state.next += 1;
            let (run, effects) = Run::start(id.clone(), task.clone(), env, self.shared.clock.now());
            state.live.insert(
                id.clone(),
                LiveRun {
                    control,
                    view: RunView {
                        phase: run.phase.clone(),
                        task,
                        title: String::new(),
                        budget: BudgetBody {
                            limits: run.task.budget,
                            used: run.used,
                        },
                    },
                    task: None,
                },
            );
            if state.pump.is_none() {
                state.pump = Some(tokio::spawn(pump(self.shared.clone())));
            }
            (run, effects)
        };
        let id = run.id.clone();
        self.shared.tell(SupervisorEvent::Added(id.clone()));
        let handle = tokio::spawn(drive(self.shared.clone(), run, effects, inbox));
        if let Some(live) = self
            .shared
            .state
            .lock()
            .expect("supervisor lock")
            .live
            .get_mut(&id)
        {
            live.task = Some(handle);
        }
        Ok(id)
    }

    fn send(&self, run: &RunId, input: RunInput) -> Result<(), ControlRefusal> {
        let state = self.shared.state.lock().expect("supervisor lock");
        let live = state.live.get(run).ok_or(ControlRefusal::WrongState)?;
        live.control
            .send(input)
            .map_err(|_| ControlRefusal::WrongState)
    }

    /// A control call on one run: cancel, pause, resume, take over, hand back.
    pub async fn control(
        &self,
        caller: CuaCaller,
        run: &RunId,
        member: CuaMember,
    ) -> Result<(), ControlRefusal> {
        guard(caller, member, self.shared.config.settings.enabled)?;
        let view = self.view(run).ok_or(ControlRefusal::WrongState)?;
        let input = input_of(member).ok_or(ControlRefusal::WrongState)?;
        if !applies(member, &view.phase) {
            return Err(ControlRefusal::WrongState);
        }
        self.send(run, input)
    }

    /// The shell's answer to a question the model asked.
    pub async fn answer(
        &self,
        caller: CuaCaller,
        run: &RunId,
        question: QuestionId,
        answer: Labelled<String>,
    ) -> Result<(), ControlRefusal> {
        guard(
            caller,
            CuaMember::Answer,
            self.shared.config.settings.enabled,
        )?;
        let view = self.view(run).ok_or(ControlRefusal::WrongState)?;
        match view.phase {
            RunPhase::WaitingForUser(_, asked) if asked == question => {
                self.send(run, RunInput::UserAnswer(question, answer))
            }
            _ => Err(ControlRefusal::UnknownQuestion),
        }
    }

    /// Every run is cancelled (the kill chord and the control centre).
    pub async fn stop_all(&self, caller: CuaCaller) -> Result<(), ControlRefusal> {
        guard(
            caller,
            CuaMember::StopAll,
            self.shared.config.settings.enabled,
        )?;
        let state = self.shared.state.lock().expect("supervisor lock");
        for live in state.live.values() {
            let _ = live
                .control
                .send(RunInput::Desktop(cua_desktop::DesktopEvent::KillChord));
        }
        Ok(())
    }
}

/// The desktop's events, for every run: each machine keeps what is its own (its lease, its
/// window) and the kill chord is everyone's.
async fn pump<S: Drivable>(shared: Arc<Shared<S>>) {
    loop {
        let event = shared.desktop.next_event().await;
        let state = shared.state.lock().expect("supervisor lock");
        for live in state.live.values() {
            let _ = live.control.send(RunInput::Desktop(event.clone()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_ids_are_r_and_the_count_and_valid_ids() {
        assert_eq!(mint_run_id(1).as_str(), "r-1");
        assert_eq!(mint_run_id(4096).as_str(), "r-4096");
        assert_ne!(mint_run_id(1), mint_run_id(2));
    }
}
