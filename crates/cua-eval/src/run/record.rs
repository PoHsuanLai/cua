//! What is kept of one run, and how it is read off the event log and the gate's notes.

use super::arena::Arena;
use crate::gate::{GateNote, Ruling};
use crate::stage::Exposure;
use crate::verdict::{Asks, Ended, Evidence, Verdict, verdict};
use cua_bus::{ActionResult, CuaRecord};
use cua_fake::MemoryEvents;
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;

/// What a step's action came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepEnd {
    /// It ran.
    Ran,
    /// The gate refused it, or cua did.
    Refused,
    /// The person declined it.
    Declined,
    /// The input could not be delivered.
    Failed,
    /// Anything else.
    Other,
}

/// What the gate did about a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Safety {
    /// Nothing: the step ran on the default rules.
    Quiet,
    /// A safety rule fired and the person was asked.
    Asked,
}

/// Whether the screen check found anything in a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScreenCheck {
    /// Nothing: no record of a suspicious screen.
    #[default]
    Quiet,
    /// The check found text aimed at the assistant (or hidden or unseen text) at least once.
    Flagged,
}

/// One step of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepTrace {
    /// Which step.
    pub n: u32,
    /// The action, as a line of a script.
    pub action: String,
    /// The effect cua named.
    pub effect: String,
    /// How it came to.
    pub end: StepEnd,
    /// Whether a safety rule fired.
    pub safety: Safety,
    /// How long the model took, in milliseconds (wall time; never asserted on).
    pub latency_ms: u64,
}

/// What is kept of one run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// The task.
    pub task: String,
    /// Which script or model played it.
    pub player: String,
    /// What became of it.
    pub verdict: Verdict,
    /// Its steps.
    pub steps: Vec<StepTrace>,
    /// How often the person was asked.
    pub asks: Asks,
    /// Which forbidden outcomes happened (positions in the task's list).
    pub violated: Vec<usize>,
    /// Whether the text an injection task plants was ever on the screen.
    pub exposure: Exposure,
    /// Whether the screen check found anything.
    #[serde(default)]
    pub screen: ScreenCheck,
    /// Every action the model proposed, as lines of a script: the run kept as a cassette.
    pub moves: Vec<String>,
    /// What the model said of each step, in a line.
    pub thoughts: Vec<String>,
    /// How the run ended, in a word.
    pub ended: String,
}

pub(super) fn record(
    arena: &Arena,
    player: &str,
    ended: Ended,
    events: &MemoryEvents,
    latencies: &[u64],
    said: &str,
) -> RunRecord {
    let log = arena.log.lock().expect("gate log lock").clone();
    let records = events.records();
    let steps: Vec<StepTrace> = records
        .iter()
        .filter_map(|r| match r {
            CuaRecord::Step {
                n,
                action,
                effect,
                result,
                ..
            } => Some((n.0, action.action.clone(), *effect, result.clone())),
            _ => None,
        })
        .enumerate()
        .map(|(i, (n, action, effect, result))| StepTrace {
            n,
            action: crate::script::line_of(&action),
            effect: effect.slug().to_owned(),
            end: step_end(&result),
            safety: safety_of(&log, n),
            latency_ms: latencies.get(i).copied().unwrap_or(0),
        })
        .collect();
    let model_asks = records
        .iter()
        .filter(|r| matches!(r, CuaRecord::Asked { .. }))
        .count();
    let forced = arena.forced.load(Ordering::Relaxed);
    let asks = Asks {
        gate: u32::try_from(log.iter().filter(|g| g.person.is_some()).count())
            .unwrap_or(u32::MAX)
            .saturating_add(forced),
        model: u32::try_from(model_asks)
            .unwrap_or(u32::MAX)
            .saturating_sub(forced),
    };
    let screen = match records
        .iter()
        .any(|r| matches!(r, CuaRecord::ScreenSuspicious { .. }))
    {
        true => ScreenCheck::Flagged,
        false => ScreenCheck::Quiet,
    };
    let evidence = Evidence {
        ended,
        success: arena.success(),
        answered: arena.answered(said),
        forbidden: arena.forbidden(),
        asks,
    };
    RunRecord {
        task: arena.task.id.clone(),
        player: player.to_owned(),
        verdict: verdict(&arena.task, evidence),
        steps,
        asks,
        violated: arena.stage.violated(),
        exposure: arena.stage.exposure(),
        screen,
        moves: Vec::new(),
        thoughts: Vec::new(),
        ended: ended_word(ended),
    }
}

fn step_end(result: &ActionResult) -> StepEnd {
    match result {
        ActionResult::Done | ActionResult::Answered(_) => StepEnd::Ran,
        ActionResult::Refused(_) | ActionResult::NotExecuted => StepEnd::Refused,
        ActionResult::UserDeclined => StepEnd::Declined,
        ActionResult::Failed(_) => StepEnd::Failed,
        ActionResult::UserActed { .. } | ActionResult::CheckFailed => StepEnd::Other,
    }
}

fn safety_of(log: &[GateNote], n: u32) -> Safety {
    let fired = log
        .iter()
        .any(|g| g.step == n && matches!(g.ruling, Ruling::Ask(_)));
    match fired {
        true => Safety::Asked,
        false => Safety::Quiet,
    }
}

fn ended_word(ended: Ended) -> String {
    match ended {
        Ended::Finished(outcome) => outcome_word(&outcome),
        Ended::Asked => "asked".to_owned(),
        Ended::Stuck => "stuck".to_owned(),
        Ended::TooLong => "too_long".to_owned(),
    }
}

/// The outcome as `kind` or `kind:detail` (`budget_out:stall`).
fn outcome_word(outcome: &cua_bus::CuaOutcome) -> String {
    match serde_json::to_value(outcome) {
        Ok(serde_json::Value::Object(map)) => {
            let kind = map.get("kind").and_then(|k| k.as_str()).unwrap_or("?");
            match map.get("v").and_then(|v| v.as_str()) {
                Some(detail) => format!("{kind}:{detail}"),
                None => kind.to_owned(),
            }
        }
        Ok(serde_json::Value::String(word)) => word,
        _ => "?".to_owned(),
    }
}
