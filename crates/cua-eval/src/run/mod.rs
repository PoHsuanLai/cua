//! One task played to its end against one model, and what is kept of it.

mod arena;
mod record;
mod timed;

pub use arena::Arena;
pub use record::{RunRecord, Safety, ScreenCheck, StepEnd, StepTrace};
pub use timed::{Stopwatch, Timed};

use crate::gate::{EvalGate, Person};
use crate::surface::SurfaceError;
use crate::verdict::Ended;
use crate::world::{EvalA11y, EvalCapture, EvalClock, EvalDesktop, EvalSeams, EvalSeat};
use cua_desktop::{BackendCaps, BackendKind};
use cua_fake::MemoryEvents;
use cua_run::{
    Clock, Collected, CuaConfig, CuaModel, CuaSignal, DamageWanted, Exec, FORCED_NO, FORCED_YES,
    Held, Run, RunEffect, RunEnv, RunInput, Seam,
};
use prov::{Label, Labelled, RunId};
use std::collections::VecDeque;
use std::sync::atomic::Ordering;

/// The most inputs a run takes before the eval gives up on it.
const MAX_INPUTS: usize = 4_000;

/// Why a task could not be set up.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The surface or its start screen.
    #[error(transparent)]
    Surface(#[from] SurfaceError),
    /// An id the fakes refuse.
    #[error("{0}")]
    Id(String),
}

/// Plays the arena's task with `model` and keeps what happened under the name `player`, its
/// steps timed by `stopwatch`.
pub async fn play<M: CuaModel + 'static>(
    arena: &Arena,
    model: M,
    player: &str,
    stopwatch: Stopwatch,
) -> Result<RunRecord, SetupError> {
    let desktop =
        EvalDesktop::new(arena.stage.clone()).map_err(|e| SetupError::Id(e.to_string()))?;
    let seat = EvalSeat::new(arena.stage.clone(), arena.clock.clone());
    let capture = EvalCapture::new(arena.stage.clone(), arena.clock.clone());
    let a11y = EvalA11y::new(arena.stage.clone(), arena.clock.clone());
    let gate = EvalGate::new(arena.task.person, arena.log.clone());
    let events = MemoryEvents::new();
    let seam = Seam::<EvalSeams<Timed<M>>> {
        desktop: &desktop,
        seat: &seat,
        capture: &capture,
        a11y: &a11y,
        gate: &gate,
        events: &events,
        clock: &arena.clock,
    };
    let env = RunEnv::new(
        &CuaConfig::default(),
        BackendCaps::of(BackendKind::KwinStock),
    );
    let id = RunId::parse("r-1").map_err(|e| SetupError::Id(e.to_string()))?;
    let (run, _) = Run::start(id, arena.cua_task()?, env, arena.clock.now());
    let mut model = Timed::new(model, stopwatch);
    let ended = drive(run, seam, &mut model, arena).await;
    let latencies = std::mem::take(&mut model.took);
    let said = std::mem::take(&mut model.said);
    let mut kept = record::record(arena, player, ended, &events, &latencies, &said);
    kept.moves = std::mem::take(&mut model.moves);
    kept.thoughts = std::mem::take(&mut model.thoughts);
    Ok(kept)
}

/// Carries the run's effects out and feeds the answers back until it ends.
async fn drive<M: CuaModel + 'static>(
    run: Run,
    seam: Seam<'_, EvalSeams<Timed<M>>>,
    model: &mut Timed<M>,
    arena: &Arena,
) -> Ended {
    let mut exec = Exec::<EvalSeams<Timed<M>>>::new(seam, run.id.clone(), run.task.clone())
        .with_hints(arena.hints.clone());
    let mut effects: VecDeque<RunEffect> = VecDeque::from([RunEffect::Grant {
        app: run.task.app.clone(),
        space: run.task.space.clone(),
        level: run.env.grant_level,
    }]);
    let mut run = run;
    let mut out = Collected::default();
    for _ in 0..MAX_INPUTS {
        while let Some(effect) = effects.pop_front() {
            exec.carry_out(effect, Some(&mut *model), &mut out).await;
        }
        let asked = out.signals.drain(..).find_map(|s| match s {
            CuaSignal::AskUser(id, _) => Some(id),
            _ => None,
        });
        if let Some(id) = asked {
            if run.track.held == Held::Nothing {
                return Ended::Asked;
            }
            // The run asked for a sensitive window: the person answers as in every
            // confirmation of the task.
            arena.forced.fetch_add(1, Ordering::Relaxed);
            let answer = match arena.task.person {
                Person::Approves => FORCED_YES,
                Person::Declines => FORCED_NO,
            };
            out.inputs.push_back(RunInput::UserAnswer(
                id,
                Labelled::new(answer.to_owned(), Label::trusted_user()),
            ));
        }
        let input = match out.inputs.pop_front() {
            Some(input) => input,
            None => match run.phase.outcome() {
                Some(outcome) => return Ended::Finished(outcome),
                None => match idle(&run, &mut exec, &arena.clock).await {
                    Some(input) => input,
                    None => return Ended::Stuck,
                },
            },
        };
        let (next, fx) = run.step(input, arena.clock.now());
        run = next;
        effects.extend(fx);
    }
    Ended::TooLong
}

/// What the world does when nothing is queued: a screen that changes, or time that passes.
async fn idle<M: CuaModel + 'static>(
    run: &Run,
    exec: &mut Exec<'_, EvalSeams<Timed<M>>>,
    clock: &EvalClock,
) -> Option<RunInput> {
    if run.track.settle.wants_damage() == DamageWanted::Yes
        && let Some(damage) = exec.next_damage().await
    {
        return Some(RunInput::Damage(damage));
    }
    let wake = run.wake()?;
    let now = clock.now();
    if wake > now {
        clock.0.advance(wake.0 - now.0);
    }
    Some(RunInput::Tick)
}
