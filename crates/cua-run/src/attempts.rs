//! What earlier attempts at a task did, in the words the model is told: the actions and how they
//! ended, never a coordinate, never typed text and never a thought. Pure.

use crate::machine::finish::said_of;
use crate::progress::Effect;
use crate::turn::sent_input;
use cua_action::{CuaAction, WindowSpace};
use cua_bus::{ActionResult, CuaOutcome, CuaRecord, LocalRefusal, Refusal};
use porter_core::AppName;
use prov::RunId;
use serde::{Deserialize, Serialize};

/// What an action is called in the words to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verb {
    /// A click.
    Click,
    /// A pointer move.
    PointerMove,
    /// A drag.
    Drag,
    /// A scroll.
    Scroll,
    /// Typing.
    Typing,
    /// A key press.
    KeyPress,
    /// A wait.
    Wait,
}

impl Verb {
    /// The verb of `action`; none for the ones that touch nothing (look, zoom, finish, ask).
    pub fn of(action: &CuaAction<WindowSpace>) -> Option<Verb> {
        match action {
            CuaAction::Click { .. } => Some(Verb::Click),
            CuaAction::MoveTo { .. } => Some(Verb::PointerMove),
            CuaAction::Drag { .. } => Some(Verb::Drag),
            CuaAction::Scroll { .. } => Some(Verb::Scroll),
            CuaAction::Type { .. } => Some(Verb::Typing),
            CuaAction::Key { .. } => Some(Verb::KeyPress),
            CuaAction::Wait { .. } => Some(Verb::Wait),
            _ => None,
        }
    }

    /// The word.
    pub fn words(self) -> &'static str {
        match self {
            Verb::Click => "click",
            Verb::PointerMove => "pointer move",
            Verb::Drag => "drag",
            Verb::Scroll => "scroll",
            Verb::Typing => "typing",
            Verb::KeyPress => "key press",
            Verb::Wait => "wait",
        }
    }
}

/// How one action ended, in the model's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mark {
    /// It ran, or nothing is held against it.
    Done,
    /// It changed nothing on screen.
    NoChange,
    /// It was refused.
    Refused,
    /// It failed.
    Failed,
    /// It never ran.
    NotRun,
}

impl Mark {
    /// What is said after the verb; nothing when it was plainly done.
    fn words(self) -> Option<&'static str> {
        match self {
            Mark::Done => None,
            Mark::NoChange => Some("no visible change"),
            Mark::Refused => Some("refused"),
            Mark::Failed => Some("failed"),
            Mark::NotRun => Some("not run"),
        }
    }
}

/// One thing tried: the verb and how it ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Tried {
    /// What it was.
    pub verb: Verb,
    /// How it ended.
    pub mark: Mark,
}

impl Tried {
    /// `action` and how it ended. `effect` is what the window showed after it (a stored step's
    /// comes from the `cua.step.seen` that follows it).
    pub fn of(
        action: &CuaAction<WindowSpace>,
        result: &ActionResult,
        effect: Effect,
    ) -> Option<Tried> {
        let verb = Verb::of(action)?;
        let mark = match result {
            ActionResult::Done if sent_input(action) && effect == Effect::NoVisibleChange => {
                Mark::NoChange
            }
            ActionResult::Done | ActionResult::Answered(_) | ActionResult::UserActed { .. } => {
                Mark::Done
            }
            ActionResult::NotExecuted => Mark::NotRun,
            ActionResult::Refused(Refusal::Local(LocalRefusal::NoProgress)) => Mark::NoChange,
            ActionResult::Refused(_) | ActionResult::UserDeclined => Mark::Refused,
            ActionResult::Failed(_) | ActionResult::CheckFailed => Mark::Failed,
        };
        Some(Tried { verb, mark })
    }
}

/// The most groups of steps one line names; older ones are cut with an ellipsis.
const MAX_GROUPS: usize = 8;

/// The steps in short words: "click, drag ×3 (no visible change)". Neighbours that match are
/// one group; a long run keeps the newest groups.
pub fn steps_words(tried: &[Tried]) -> String {
    let mut groups: Vec<(Tried, usize)> = Vec::new();
    for step in tried {
        match groups.last_mut() {
            Some((last, count)) if last == step => *count += 1,
            _ => groups.push((*step, 1)),
        }
    }
    let cut = groups.len().saturating_sub(MAX_GROUPS);
    let words: Vec<String> = groups
        .iter()
        .skip(cut)
        .map(|(step, count)| {
            let times = if *count > 1 {
                format!(" ×{count}")
            } else {
                String::new()
            };
            let mark = step
                .mark
                .words()
                .map_or_else(String::new, |w| format!(" ({w})"));
            format!("{}{times}{mark}", step.verb.words())
        })
        .collect();
    let lead = if cut > 0 { "… " } else { "" };
    format!("{lead}{}", words.join(", "))
}

/// What a stalled run tells its planner: the screen stopped changing, and what was tried.
pub fn stall_words(tried: &[Tried]) -> String {
    let base = said_of(CuaOutcome::BudgetOut(cua_bus::RunBudgetKind::Stall));
    if tried.is_empty() {
        return base.to_owned();
    }
    let n = tried.len();
    let plural = if n == 1 { "" } else { "s" };
    format!(
        "The screen stopped changing after {n} step{plural}: {}.",
        steps_words(tried)
    )
}

/// How an attempt ended, in a sentence.
fn end_words(outcome: CuaOutcome) -> &'static str {
    match (outcome, said_of(outcome)) {
        (CuaOutcome::Done, _) => "It said it was done.",
        (_, "") => "It was stopped.",
        (_, said) => said,
    }
}

/// How much of the past a briefing keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The newest attempts kept.
    pub attempts: usize,
    /// The lines in all; the oldest attempts go first.
    pub lines: usize,
}

impl Limits {
    /// What the model is told: three attempts, twelve lines.
    pub const BRIEFING: Limits = Limits {
        attempts: 3,
        lines: 12,
    };
}

/// One earlier run, as far as its records say.
struct Attempt {
    run: RunId,
    tried: Vec<Tried>,
    /// The actions since the last look, as they read if that look shows no change: where each
    /// sits in `tried`, and its words then.
    unseen: Vec<(usize, Tried)>,
    outcome: Option<CuaOutcome>,
}

impl Attempt {
    /// A step's action: it reads as done until a look says otherwise.
    fn step(&mut self, action: &CuaAction<WindowSpace>, result: &ActionResult) {
        let Some(tried) = Tried::of(action, result, Effect::Changed) else {
            return;
        };
        if let Some(unchanged) = Tried::of(action, result, Effect::NoVisibleChange) {
            self.unseen.push((self.tried.len(), unchanged));
        }
        self.tried.push(tried);
    }

    /// The look after the actions since the last one, as the run itself judged them.
    fn seen(&mut self, effect: Effect) {
        for (i, unchanged) in std::mem::take(&mut self.unseen) {
            if effect == Effect::NoVisibleChange {
                self.tried[i] = unchanged;
            }
        }
    }
}

impl Attempt {
    fn lines(&self, k: usize) -> Vec<String> {
        let steps = if self.tried.is_empty() {
            "no actions".to_owned()
        } else {
            steps_words(&self.tried)
        };
        let mut lines = vec![format!("Attempt {k}:"), format!("Tried: {steps}.")];
        lines.extend(self.outcome.map(|o| end_words(o).to_owned()));
        lines
    }
}

/// The briefing's lines about earlier runs in `app`, oldest first, from the conversation's
/// records; `current` is left out. See [`digest_within`].
pub fn digest(records: &[CuaRecord], app: &AppName, current: Option<&RunId>) -> Vec<String> {
    digest_within(records, app, current, Limits::BRIEFING)
}

/// [`digest`] with its limits given: runs are grouped by the run their records name, a run
/// counts when its start names `app`, the newest `limits.attempts` stay, and whole attempts are
/// dropped from the oldest end until the lines fit.
pub fn digest_within(
    records: &[CuaRecord],
    app: &AppName,
    current: Option<&RunId>,
    limits: Limits,
) -> Vec<String> {
    let mut attempts: Vec<Attempt> = Vec::new();
    for record in records {
        match record {
            CuaRecord::RunStarted { run, app: of, .. } if of == app && Some(run) != current => {
                attempts.push(Attempt {
                    run: run.clone(),
                    tried: Vec::new(),
                    unseen: Vec::new(),
                    outcome: None,
                });
            }
            CuaRecord::Step {
                run,
                action,
                result,
                ..
            } => {
                if let Some(attempt) = attempts.iter_mut().find(|a| &a.run == run) {
                    attempt.step(&action.action, result);
                }
            }
            CuaRecord::Seen { run, effect, .. } => {
                if let Some(attempt) = attempts.iter_mut().find(|a| &a.run == run) {
                    attempt.seen(*effect);
                }
            }
            CuaRecord::Finished { run, outcome, .. } => {
                if let Some(attempt) = attempts.iter_mut().find(|a| &a.run == run) {
                    attempt.outcome = Some(*outcome);
                }
            }
            _ => {}
        }
    }
    let first = attempts.len().saturating_sub(limits.attempts);
    let mut blocks: Vec<Vec<String>> = attempts
        .iter()
        .enumerate()
        .skip(first)
        .map(|(i, attempt)| attempt.lines(i + 1))
        .collect();
    while blocks.iter().map(Vec::len).sum::<usize>() > limits.lines && !blocks.is_empty() {
        blocks.remove(0);
    }
    blocks.into_iter().flatten().collect()
}
