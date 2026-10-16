//! The run itself: its state, and the three functions that drive it.

use crate::chord_scope::KeyWorld;
use crate::config::{Autopause, CuaConfig, SettleLimits};
use crate::machine::effect::RunEffect;
use crate::machine::input::RunInput;
use crate::machine::phase::{ActionQueue, RunPhase};
use crate::machine::step;
use crate::machine::track::RunTrack;
use crate::observation::Observation;
use crate::stall::Repeats;
use a11y_tree::NodeBudget;
use cua_bus::{BudgetUse, CuaTask, RunStateSlug};
use cua_desktop::{BackendCaps, LeaseId, Ms, Stamp};
use docket_core::Level;
use porter_infer::ServedBy;
use prov::{Label, RunId};
use serde::{Deserialize, Serialize};

/// What the machine needs to know about the world it runs in, fixed for the run: what the
/// backend can do and the settings that shape the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEnv {
    /// The backend's capabilities (consent guard, seat isolation, masking).
    pub caps: BackendCaps,
    /// How long to wait for the screen to settle.
    pub settle: SettleLimits,
    /// The longest a model may ask to wait.
    pub wait_max: Ms,
    /// Whether physical input pauses a run on a shared seat.
    pub autopause: Autopause,
    /// How many tree nodes a snapshot keeps.
    pub tree_max_nodes: NodeBudget,
    /// The key conventions of the desktop, to tell a chord for the desktop from one for the
    /// window.
    #[serde(default)]
    pub keys: KeyWorld,
    /// The consent level the person set for this run's app (`cua.apps`), sent with the grant.
    /// The supervisor decides it; the caller's task never carries it.
    #[serde(default = "strictest_grant")]
    pub grant_level: Level,
}

/// The level of a run whose supervisor did not say: ask every time.
fn strictest_grant() -> Level {
    Level::AlwaysAsk
}

impl RunEnv {
    /// The environment of a run under `config` on a backend with `caps`.
    pub fn new(config: &CuaConfig, caps: BackendCaps) -> RunEnv {
        RunEnv {
            caps,
            settle: config.settle,
            wait_max: config.wait_max,
            autopause: config.autopause,
            tree_max_nodes: config.tree_max_nodes,
            keys: KeyWorld::default(),
            grant_level: strictest_grant(),
        }
    }

    /// The same environment for an app the person allowed at `grant_level`.
    pub fn with_grant_level(self, grant_level: Level) -> RunEnv {
        RunEnv {
            grant_level,
            ..self
        }
    }

    /// The same environment on a desktop with these key conventions.
    pub fn with_keys(self, keys: KeyWorld) -> RunEnv {
        RunEnv { keys, ..self }
    }
}

/// One run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    /// Its id.
    pub id: RunId,
    /// What it was asked.
    pub task: CuaTask,
    /// The world it runs in.
    pub env: RunEnv,
    /// Where it is.
    pub phase: RunPhase,
    /// What it has used.
    pub used: BudgetUse,
    /// What it has read: always `Untrusted(Screen)` once it has seen the window, and the join of
    /// every extracted value's label. Reports carry it.
    pub taint: Label,
    /// The newest frame's digest and how often it has come in a row, for stall detection (no
    /// pixels).
    pub frames: Repeats,
    /// The model's batch that has not run yet.
    pub pending: ActionQueue,
    /// The lease it holds, once it has one.
    pub lease: Option<LeaseId>,
    /// Who answers its model calls, once inferd has said.
    pub served: Option<ServedBy>,
    /// The observation the current step works from.
    pub working: Option<Box<Observation>>,
    /// The last instant the machine saw: active time is counted between instants.
    pub last_at: Stamp,
    /// The machine's own notes: the step's outcomes so far, the settle watch, the rate window.
    pub track: RunTrack,
}

impl Run {
    /// A run, started: `Granting`, with the effect that asks the router for the per-app consent.
    /// A mode the backend cannot run (`mode_support`) finishes it at once as
    /// `Blocked(ModeUnsupported(mode))`, and a target of `TaskTarget::App` as
    /// `Blocked(NeedsCapability)` until launching into a mode lands.
    pub fn start(id: RunId, task: CuaTask, env: RunEnv, at: Stamp) -> (Run, Vec<RunEffect>) {
        step::start(id, task, env, at)
    }

    /// One transition: the next run and the effects to carry out. Every input in every phase has
    /// a row; `Finished` ignores every input (same run, no effects). Active time is counted from
    /// `at` against the previous instant, and only in active phases.
    pub fn step(mut self, input: RunInput, at: Stamp) -> (Run, Vec<RunEffect>) {
        let effects = self.advance(input, at);
        (self, effects)
    }

    /// The same transition in place: the run is now the next run, and the effects to carry out
    /// come back.
    pub fn advance(&mut self, input: RunInput, at: Stamp) -> Vec<RunEffect> {
        step::advance(self, input, at)
    }

    /// The next instant the machine wants a `Tick`: a settle wake-up or the active-budget
    /// deadline; none when it is waiting for the world.
    pub fn wake(&self) -> Option<Stamp> {
        step::wake(self)
    }

    /// The bus slug of the current phase.
    pub fn slug(&self) -> RunStateSlug {
        self.phase.slug()
    }
}
