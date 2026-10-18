//! The supervisor: every live run, the lease book, and the control calls on them.

use crate::config::CuadConfig;
use crate::drive::{Drivable, LiveRun, Shared, drive};
use crate::events::{RunView, SupervisorEvent};
use crate::guard::guard;
use crate::settings::{Apps, CuaSettings, Loaded, SettingsFeed};
use cua_bus::{
    BudgetBody, ControlRefusal, CuaCaller, CuaMember, CuaTask, ModeSupport, QuestionId, TaskTarget,
};
use cua_desktop::Desktop;
use cua_run::{
    Clock, CuaConfig, CuaModel, Enabled, GateLink, LeaseBook, ModelReach, ModelSight,
    NO_SIGHT_NOTE, Parts, Run, RunEnv, RunInput, RunPhase, Seams, mode_support,
};
use prov::{Integrity, Labelled, ModelRole, RunId, Source};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};

/// The id of the `n`th run cuad starts: `r-<n>`.
pub fn mint_run_id(n: u64) -> RunId {
    RunId::parse(&format!("r-{n}")).expect("`r-` and digits are an id")
}

/// `task` with everything the planner wrote marked as the planner's: its goal and the values it
/// passes in carry no more trust than the planner has, whatever label they came with.
fn untrusted_from_planner(mut task: CuaTask) -> CuaTask {
    let mark = |label: &mut prov::Label| {
        label.integrity = Integrity::Untrusted;
        label.sources.insert(Source::Model(ModelRole::Planner));
    };
    mark(&mut task.goal.label);
    for value in &mut task.data_in {
        mark(&mut value.label);
    }
    task
}

/// Owns the runs. Each run is one task over one `Run` machine; the supervisor carries out its
/// effects against the seams and feeds the answers back.
///
/// The model is one seam for all runs and holds one session, so a run keeps it while it works
/// and lets it go while it is paused, taken over or waiting for the person (it opens a new
/// session when it goes on): runs on different windows work side by side, but their model turns
/// take turns, one run at a time (FINDINGS, upstream asks).
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
        let mut state = self.shared.state();
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
        // Not run controls, and any member this build does not know: nothing applies.
        _ => false,
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
        // Not run controls, and any member this build does not know: no input.
        _ => None,
    }
}

impl<S: Seams> Supervisor<S> {
    /// A supervisor over `parts` under `config`.
    pub fn new(parts: Parts<S>, config: CuadConfig) -> Self {
        Self::with_hints(parts, config, Vec::new())
    }

    /// A supervisor whose runs tell their model `hints` about the desktop when the session opens
    /// (the shortcut line, `cua_run::shortcut_hint`, for one). The caller builds them once at
    /// start; nothing here reads an environment.
    pub fn with_hints(parts: Parts<S>, config: CuadConfig, hints: Vec<String>) -> Self {
        Self {
            shared: Arc::new(Shared::of(parts, config, hints)),
        }
    }

    /// The configuration cuad started with.
    pub fn config(&self) -> &CuadConfig {
        &self.shared.config
    }

    fn live(&self) -> CuaSettings {
        match self.shared.settings.read() {
            Ok(live) => live.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }

    /// The `cua.*` values in force now: the person's latest settings, else the configuration's.
    /// Every new run and every guard reads them afresh, so a change applies to the next one (a
    /// run already going keeps the values it started with).
    pub fn settings(&self) -> CuaConfig {
        self.live().config
    }

    /// The apps computer use may drive now.
    pub fn apps(&self) -> Apps {
        self.live().apps
    }

    /// Puts the person's settings in force for every call after this one.
    pub fn apply_settings(&self, settings: &CuaSettings) {
        match self.shared.settings.write() {
            Ok(mut live) => *live = settings.clone(),
            Err(poisoned) => *poisoned.into_inner() = settings.clone(),
        }
    }

    /// Waits for the next change of the settings file and applies it; `None` when the watch ended.
    pub async fn follow(&self, watch: &mut impl SettingsFeed) -> Option<Loaded> {
        let loaded = watch.changed().await?;
        for line in loaded.lines("cuad") {
            eprintln!("{line}");
        }
        self.apply_settings(&loaded.value);
        Some(loaded)
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
        self.shared.state().leases.clone()
    }

    /// How many runs have been started (the next id is `r-<this + 1>`).
    pub fn started(&self) -> u64 {
        self.shared.state().next
    }

    /// The ids of the runs now.
    pub fn runs(&self) -> Vec<RunId> {
        self.shared.state().live.keys().cloned().collect()
    }

    /// What the bus shows of one live run.
    pub fn view(&self, run: &RunId) -> Option<RunView> {
        self.shared
            .state()
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
    /// Whether a model can serve a new run. A model a run is working with is plainly there, so
    /// only an idle one is asked.
    async fn model_reach(&self) -> ModelReach {
        match self.shared.model.try_lock() {
            Ok(mut model) => model.reach().await,
            Err(_) => ModelReach::Serves,
        }
    }

    /// What the shell says beside computer use about the model: the sentence when computer use
    /// is on and every model that could serve is known not to read images, and nothing
    /// otherwise. A model a run is working with is plainly in use, so only an idle one is asked.
    pub async fn sight_note(&self) -> Option<&'static str> {
        if self.settings().enabled != Enabled::On {
            return None;
        }
        let sight = match self.shared.model.try_lock() {
            Ok(mut model) => model.sight().await,
            Err(_) => ModelSight::MaySee,
        };
        match sight {
            ModelSight::MaySee => None,
            ModelSight::Blind => Some(NO_SIGHT_NOTE),
        }
    }

    /// Starts a run for `task` (`guard` first; one run per window; the mode must be supported).
    pub async fn start(
        &self,
        caller: CuaCaller,
        mut task: CuaTask,
    ) -> Result<RunId, ControlRefusal> {
        guard(caller, CuaMember::Start, self.settings().enabled)?;
        // Computer use is on per app (`cua.apps`): an app the person has not listed is not on.
        let Some(allow) = self.apps().rule_for(&task.app) else {
            return Err(ControlRefusal::Disabled);
        };
        let caps = self.shared.desktop.caps();
        if let ModeSupport::Unsupported(_) = mode_support(&caps, task.mode) {
            return Err(ControlRefusal::ModeUnsupported);
        }
        // A run needs a model that can serve it: refused here, in words, instead of failing
        // later where the person cannot see it.
        if self.model_reach().await == ModelReach::Missing {
            return Err(ControlRefusal::ModelUnavailable);
        }
        if let TaskTarget::Window(ident) = &task.target {
            let known = self
                .shared
                .desktop
                .windows()
                .await
                .map_err(|_| ControlRefusal::WindowUnknown)?;
            let window = known
                .iter()
                .find(|w| &w.ident == ident)
                .ok_or(ControlRefusal::WindowUnknown)?;
            // The app is the window's, not the caller's word: consent and the allow-list were
            // checked for `task.app`, so they must be for the app that owns the window.
            if window.app != task.app {
                return Err(ControlRefusal::BadTask);
            }
        }
        // The caller's body is a request, not a grant: limits stay within the person's own,
        // and a planner's words never count as the person's.
        task.budget = task.budget.capped_by(self.settings().budget);
        if caller == CuaCaller::Companion {
            task = untrusted_from_planner(task);
        }
        let env = RunEnv::new(&self.settings(), caps).with_grant_level(allow.grant_level());
        let (control, inbox) = mpsc::unbounded_channel();
        let id = {
            let mut state = self.shared.state();
            let id = mint_run_id(state.next + 1);
            if let TaskTarget::Window(ident) = &task.target {
                state.leases = state
                    .leases
                    .claim(ident, &id)
                    .map_err(|_| ControlRefusal::LeaseHeld)?;
            }
            state.next += 1;
            id
        };
        // The run acts in a session of its own, opened before its first check: the router
        // finds the run's rules by it, and the run's reports go out as the run.
        match self.shared.gate.open_session(&id, &task.space).await {
            Ok(session) => task.session = session,
            Err(_) => {
                let mut state = self.shared.state();
                state.leases = state.leases.release(&id);
                return Err(ControlRefusal::NotPermitted);
            }
        }
        let (run, effects) = {
            let mut state = self.shared.state();
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
        if let Some(live) = self.shared.state().live.get_mut(&id) {
            live.task = Some(handle);
        }
        Ok(id)
    }

    fn send(&self, run: &RunId, input: RunInput) -> Result<(), ControlRefusal> {
        let state = self.shared.state();
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
        guard(caller, member, self.settings().enabled)?;
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
        guard(caller, CuaMember::Answer, self.settings().enabled)?;
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
        guard(caller, CuaMember::StopAll, self.settings().enabled)?;
        let state = self.shared.state();
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
        let state = shared.state();
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
