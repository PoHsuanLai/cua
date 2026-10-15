//! Playing the task set: every script of every task (the scripted engine, which needs no model),
//! a model on a provider (the live engine), or a cassette of a live run played back.

use crate::corpus::Corpus;
use crate::live::{Plan, ProviderModel};
use crate::report::Check;
use crate::run::{RunRecord, SetupError, Stopwatch, play};
use crate::script::ScriptModel;
use crate::task::{Engines, Served, TaskDef};
use crate::verdict::Verdict;
use model_provider::Provider;
use porter_core::{AccountId, ModelId};
use porter_infer::ServedBy;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A live run kept as a script: what the model did, step by step, and the verdict it earned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Take {
    /// The task.
    pub task: String,
    /// Who played it.
    pub player: String,
    /// The verdict the run earned.
    pub verdict: Verdict,
    /// The moves, one per line.
    pub steps: Vec<String>,
    /// What the model said of each step, for a person reading the take; a script has none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// A file of takes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Cassette {
    /// The takes.
    #[serde(rename = "run")]
    pub takes: Vec<Take>,
}

impl Cassette {
    /// The takes of `records`.
    pub fn of(records: &[RunRecord]) -> Cassette {
        Cassette {
            takes: records
                .iter()
                .map(|r| Take {
                    task: r.task.clone(),
                    player: r.player.clone(),
                    verdict: r.verdict,
                    steps: r.moves.clone(),
                    notes: r.thoughts.clone(),
                })
                .collect(),
        }
    }
}

fn picked<'a>(corpus: &'a Corpus, only: &'a [String]) -> impl Iterator<Item = &'a TaskDef> {
    corpus
        .tasks
        .iter()
        .filter(move |t| only.is_empty() || only.contains(&t.id))
}

fn served_by(served: Served) -> Result<Option<ServedBy>, SetupError> {
    match served {
        Served::Local => Ok(None),
        Served::Cloud => {
            let id = |e: &dyn std::fmt::Display| SetupError::Id(e.to_string());
            Ok(Some(ServedBy {
                account: AccountId::parse("acct-1").map_err(|e| id(&e))?,
                model: ModelId::parse("model-1").map_err(|e| id(&e))?,
                locality: porter_core::Locality::Cloud { region: None },
            }))
        }
    }
}

/// Plays every script of every picked task.
pub async fn run_scripted(
    corpus: &Corpus,
    only: &[String],
) -> Result<(Vec<RunRecord>, Vec<Check>), SetupError> {
    let mut records = Vec::new();
    let mut checks = Vec::new();
    for task in picked(corpus, only) {
        for script in &task.scripts {
            let arena = corpus.arena(task)?;
            let mut model = ScriptModel::new(arena.stage.clone(), &script.steps)
                .map_err(|e| SetupError::Id(e.to_string()))?;
            if let Some(served) = served_by(script.served)? {
                model = model.served_by(served);
            }
            let record = play(
                &arena,
                model,
                &format!("script:{}", script.name),
                Stopwatch::Still,
            )
            .await?;
            checks.push(Check {
                task: task.id.clone(),
                script: script.name.clone(),
                expect: script.expect,
                got: record.verdict,
            });
            records.push(record);
        }
    }
    Ok((records, checks))
}

/// Plays a cassette back: each take on its task, and the verdict it earned compared with the
/// one the take recorded.
pub async fn run_cassette(
    corpus: &Corpus,
    cassette: &Cassette,
    only: &[String],
) -> Result<(Vec<RunRecord>, Vec<Check>), SetupError> {
    let mut records = Vec::new();
    let mut checks = Vec::new();
    for take in &cassette.takes {
        let Some(task) = picked(corpus, only).find(|t| t.id == take.task) else {
            continue;
        };
        let arena = corpus.arena(task)?;
        let model = ScriptModel::new(arena.stage.clone(), &take.steps)
            .map_err(|e| SetupError::Id(e.to_string()))?;
        let record = play(&arena, model, &take.player, Stopwatch::Still).await?;
        checks.push(Check {
            task: task.id.clone(),
            script: take.player.clone(),
            expect: take.verdict,
            got: record.verdict,
        });
        records.push(record);
    }
    Ok((records, checks))
}

/// Plays every picked task `repeat` times against a model on `provider`.
pub async fn run_live<P: Provider + 'static>(
    corpus: &Corpus,
    provider: Arc<P>,
    plan: &Plan,
    player: &str,
    repeat: u32,
    only: &[String],
) -> Result<Vec<RunRecord>, SetupError> {
    let served = ServedBy {
        account: AccountId::parse("acct-local").map_err(|e| SetupError::Id(e.to_string()))?,
        model: ModelId::parse("model-local").map_err(|e| SetupError::Id(e.to_string()))?,
        locality: porter_core::Locality::OnDevice,
    };
    let lot = Lot {
        player,
        repeat,
        only,
        served,
        spend_microusd: None,
    };
    let (records, _) = run_live_until(corpus, provider, plan, &lot, |_: &[RunRecord]| None).await?;
    Ok(records)
}

/// Which runs a live eval plays and who it says answered.
#[derive(Debug, Clone)]
pub struct Lot<'a> {
    /// The name the runs carry.
    pub player: &'a str,
    /// How many runs of each task.
    pub repeat: u32,
    /// The tasks picked by id; all when empty.
    pub only: &'a [String],
    /// Who the model says it is.
    pub served: ServedBy,
    /// A spend budget for every task, replacing the task's own: a task's zero means "local models
    /// only" and ends a run at the first reply from a provider's servers. The run machine counts
    /// a micro-dollar a reply, so this is a cap on replies; the money is the broker's to cap.
    pub spend_microusd: Option<u64>,
}

/// [`run_live`] for a model somewhere else, that stops early: before each run `halt` is asked with
/// the runs so far, and a reason ends the whole eval there (a spend refusal, a step budget). The
/// runs played are kept; the reason comes back with them.
pub async fn run_live_until<P: Provider + 'static>(
    corpus: &Corpus,
    provider: Arc<P>,
    plan: &Plan,
    lot: &Lot<'_>,
    halt: impl Fn(&[RunRecord]) -> Option<String>,
) -> Result<(Vec<RunRecord>, Option<String>), SetupError> {
    let mut records = Vec::new();
    for task in picked(corpus, lot.only).filter(|t| t.engines == Engines::Both) {
        for _ in 0..lot.repeat {
            if let Some(why) = halt(&records) {
                return Ok((records, Some(why)));
            }
            let task = TaskDef {
                spend_microusd: lot.spend_microusd.unwrap_or(task.spend_microusd),
                ..task.clone()
            };
            let arena = corpus.arena(&task)?;
            let model = ProviderModel::new(provider.clone(), plan.clone(), lot.served.clone());
            records.push(play(&arena, model, lot.player, Stopwatch::Wall).await?);
        }
    }
    let last = halt(&records);
    Ok((records, last))
}

/// What the model sees first on each picked task, as PNG bytes under the task's id: for a person
/// to look at the surfaces, and at what a model is shown of them.
pub fn start_screens(
    corpus: &Corpus,
    only: &[String],
) -> Result<Vec<(String, Vec<u8>)>, SetupError> {
    let id = |e: &dyn std::fmt::Display| SetupError::Id(e.to_string());
    let rule = vision_prep::ResizeRule::SmartResize {
        factor: vision_prep::PatchFactor(32),
        min_pixels: vision_prep::PixelCount(65_536),
        max_pixels: vision_prep::PixelCount(16_777_216),
    };
    let mut shots = Vec::new();
    for task in picked(corpus, only) {
        let arena = corpus.arena(task)?;
        let pixels = arena.stage.draw();
        let window = cua_action::Size::new(
            cua_action::Coord(pixels.size.w),
            cua_action::Coord(pixels.size.h),
        );
        let map = vision_prep::FrameMap::new(
            window,
            cua_action::Scale120(120),
            &rule,
            cua_action::ModelSpace::Image,
        )
        .map_err(|e| id(&e))?;
        let raw = vision_prep::RawFrame {
            pixels: &pixels.data,
            size: pixels.size,
            stride: pixels.stride,
            format: pixels.format,
        };
        let png =
            vision_prep::prepare(raw, &map, vision_prep::Encoding::Png).map_err(|e| id(&e))?;
        shots.push((task.id.clone(), png.bytes));
    }
    Ok(shots)
}
