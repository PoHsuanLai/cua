//! The supervisor: every live run, the lease book, and the control calls on them.

use crate::config::CuadConfig;
use cua_bus::{ControlRefusal, CuaCaller, CuaTask, QuestionId};
use cua_run::{LeaseBook, Parts, Seams};
use prov::{Labelled, RunId};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The id of the `n`th run cuad starts: `r-<n>`.
pub fn mint_run_id(n: u64) -> RunId {
    RunId::parse(&format!("r-{n}")).expect("`r-` and digits are an id")
}

/// Owns the runs. Each run is one task over one `Run` machine; the supervisor carries out its
/// effects against the seams and feeds the answers back.
pub struct Supervisor<S: Seams> {
    parts: Parts<S>,
    config: CuadConfig,
    state: Mutex<SupervisorState>,
}

/// What the supervisor keeps between calls.
#[derive(Debug, Default)]
struct SupervisorState {
    next: u64,
    leases: LeaseBook,
    live: BTreeMap<RunId, cua_run::RunPhase>,
}

impl<S: Seams> std::fmt::Debug for Supervisor<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor").finish_non_exhaustive()
    }
}

impl<S: Seams> Supervisor<S> {
    /// A supervisor over `parts` under `config`.
    pub fn new(parts: Parts<S>, config: CuadConfig) -> Self {
        Self {
            parts,
            config,
            state: Mutex::new(SupervisorState::default()),
        }
    }

    /// The configuration.
    pub fn config(&self) -> &CuadConfig {
        &self.config
    }

    /// The seams.
    pub fn parts(&self) -> &Parts<S> {
        &self.parts
    }

    /// Which run holds which window now.
    pub fn lease_book(&self) -> LeaseBook {
        self.state.lock().expect("supervisor lock").leases.clone()
    }

    /// How many runs have been started (the next id is `r-<this + 1>`).
    pub fn started(&self) -> u64 {
        self.state.lock().expect("supervisor lock").next
    }

    /// The ids of the runs now.
    pub fn runs(&self) -> Vec<RunId> {
        self.state
            .lock()
            .expect("supervisor lock")
            .live
            .keys()
            .cloned()
            .collect()
    }

    /// Starts a run for `task` (`guard` first; one run per window; the mode must be supported).
    pub async fn start(&self, caller: CuaCaller, task: CuaTask) -> Result<RunId, ControlRefusal> {
        let _ = (caller, task);
        todo!(
            "guard, LeaseBook::claim, mint the id, Run::start, spawn the effect loop (end_to_end_scripted_run_done)"
        )
    }

    /// A control call on one run: cancel, pause, resume, take over, hand back.
    pub async fn control(
        &self,
        caller: CuaCaller,
        run: &RunId,
        member: cua_bus::CuaMember,
    ) -> Result<(), ControlRefusal> {
        let _ = (caller, run, member);
        todo!("guard, then the matching RunInput")
    }

    /// The shell's answer to a question the model asked.
    pub async fn answer(
        &self,
        caller: CuaCaller,
        run: &RunId,
        question: QuestionId,
        answer: Labelled<String>,
    ) -> Result<(), ControlRefusal> {
        let _ = (caller, run, question, answer);
        todo!(
            "guard (shell only), UnknownQuestion unless the run waits on it, RunInput::UserAnswer"
        )
    }

    /// Every run is cancelled (the kill chord and the control centre).
    pub async fn stop_all(&self, caller: CuaCaller) -> Result<(), ControlRefusal> {
        let _ = caller;
        todo!("guard, RunInput::Desktop(KillChord) to every run (kill_chord_finishes_all_runs)")
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
