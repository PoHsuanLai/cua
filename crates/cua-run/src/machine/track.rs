//! What the machine remembers between inputs beyond the phase: small, typed, and never pixels or
//! thought text.

use crate::attempts::Tried;
use crate::progress::{Effect, Look};
use crate::screen::ScreenTrack;
use crate::sensitivity::WindowSensitivity;
use cua_action::{CuaAction, WindowSpace};
use cua_bus::{ActionOutcome, AuthorisedBy, FrameNote, ThoughtDigest};
use cua_desktop::{Ms, Stamp};
use maskcap::DamageAt;
use serde::{Deserialize, Serialize};

/// How often the capture of this step has been tried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureTries {
    /// Not failed yet.
    First,
    /// Failed once and tried again; the next failure ends the run.
    Retried,
}

/// What a `Settling` phase is waiting for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SettleWatch {
    /// Nothing.
    Idle,
    /// The screen to stop changing after an action that ran at `since`; `latest` is the newest
    /// damage seen.
    Damage {
        /// When the action ran.
        since: Stamp,
        /// The newest change.
        latest: Option<DamageAt>,
    },
    /// The model's `wait`: it ends at `until`, or earlier once the window has changed and then
    /// been quiet for the settle period.
    Change {
        /// The deadline.
        until: Stamp,
        /// The newest change seen.
        latest: Option<DamageAt>,
    },
}

impl SettleWatch {
    /// Whether the driver should feed this watch the window's damage.
    pub fn wants_damage(&self) -> DamageWanted {
        match self {
            SettleWatch::Damage { .. } | SettleWatch::Change { .. } => DamageWanted::Yes,
            SettleWatch::Idle => DamageWanted::No,
        }
    }
}

/// Whether a watch listens for the window's damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DamageWanted {
    /// It does.
    Yes,
    /// It does not.
    No,
}

/// How long the run has been waiting on end, `wait` after `wait`. An action that is not a wait
/// starts the count over. While it is within the run's total (`WAIT_TOTAL`) an unchanged
/// screen is a slow job and not a stall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Waited {
    /// Not waiting.
    #[default]
    No,
    /// Waiting for this long so far.
    For(Ms),
}

impl Waited {
    /// The count after one more wait of `ms`.
    #[must_use]
    pub fn and(self, ms: Ms) -> Waited {
        match self {
            Waited::No => Waited::For(ms),
            Waited::For(Ms(so_far)) => Waited::For(Ms(so_far.saturating_add(ms.0))),
        }
    }
}

/// Whether the head action waits for the person's yes because its window is sensitive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Held {
    /// Nothing is held; a question that is waiting is the model's own.
    #[default]
    Nothing,
    /// The head action waits for the person's answer to the question the run asked for it.
    Sensitive(WindowSensitivity),
    /// The run asked because the screen looks aimed at the assistant (`RunTrack::screen` says
    /// what was found); the look waits for the answer, and a yes lets the model be called.
    SuspiciousScreen,
    /// The head action sends something out or removes something, and the run has seen a
    /// suspicious screen: the person is asked first, as for a sensitive window.
    AfterSuspicion,
}

/// The machine's own notes on the current run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunTrack {
    /// The capture retry of the current step.
    pub capture: CaptureTries,
    /// How this step's actions ended so far; they become the next observation's `prev`.
    pub outcomes: Vec<ActionOutcome>,
    /// When actions ran in the last minute, for the rate limit.
    pub acted: Vec<Stamp>,
    /// What `Settling` waits for.
    pub settle: SettleWatch,
    /// Who allowed the action now being carried out.
    pub authorised: AuthorisedBy,
    /// The digest of the current step's thought, if the model gave one.
    pub thought: Option<ThoughtDigest>,
    /// What is kept of the current step's frame.
    pub frame: Option<FrameNote>,
    /// How many of the current batch's actions the model already announced while it streamed
    /// (so the reply announces only the rest).
    pub proposed: u32,
    /// Whether the question now waiting was asked by the run for a sensitive window.
    #[serde(default)]
    pub held: Held,
    /// The id of the last question asked, so that no two questions of a run share one.
    #[serde(default)]
    pub last_question: u64,
    /// How long the run has been waiting, `wait` after `wait`.
    #[serde(default)]
    pub waited: Waited,
    /// What the screen check found and what the person said of it.
    #[serde(default)]
    pub screen: ScreenTrack,
    /// The last look at the window, to tell what the last action changed.
    #[serde(default)]
    pub look: Option<Look>,
    /// What the last action did to the window, from the last two looks.
    #[serde(default)]
    pub effect: Effect,
    /// The input action that ran last, until a wait: repeating it on an unchanged screen is
    /// refused.
    #[serde(default)]
    pub last_input: Option<CuaAction<WindowSpace>>,
    /// Every action the run has tried and how it ended, for the words of a stall.
    #[serde(default)]
    pub tried: Vec<Tried>,
}

impl RunTrack {
    /// A run that has not started a step.
    pub fn new() -> RunTrack {
        RunTrack {
            capture: CaptureTries::First,
            outcomes: Vec::new(),
            acted: Vec::new(),
            settle: SettleWatch::Idle,
            authorised: AuthorisedBy::Policy,
            thought: None,
            frame: None,
            proposed: 0,
            held: Held::Nothing,
            last_question: 0,
            waited: Waited::No,
            screen: ScreenTrack::default(),
            look: None,
            effect: Effect::Changed,
            last_input: None,
            tried: Vec::new(),
        }
    }
}

impl Default for RunTrack {
    fn default() -> Self {
        RunTrack::new()
    }
}
