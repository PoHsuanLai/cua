//! A run driven over the fakes: the machine's effects carried out against them, and their
//! answers fed back, until the run ends.

use crate::a11y::FakeA11y;
use crate::capture::FakeCapture;
use crate::clock::FixedClock;
use crate::desktop::FakeDesktop;
use crate::events::MemoryEvents;
use crate::gate::FakePdp;
use crate::model::ScriptedModel;
use crate::seat::FakeSeat;
use cua_run::{
    Clock, Collected, CuaSignal, Exec, Parts, Run, RunEffect, RunInput, RunPhase, Seam, Seams,
    StepNote,
};
use std::collections::VecDeque;

/// The bundle of fakes: every seam of a run, faked.
#[derive(Debug)]
pub struct FakeSeams;

impl Seams for FakeSeams {
    type Desktop = FakeDesktop;
    type Seat = FakeSeat;
    type Capture = FakeCapture;
    type A11y = FakeA11y;
    type Model = ScriptedModel;
    type Gate = FakePdp;
    type Events = MemoryEvents;
    type Time = FixedClock;
}

/// Every fake a run needs, in one place.
pub type World = Parts<FakeSeams>;

/// What a harness run leaves behind. The records and the reports sent are in `World::events`.
#[derive(Debug)]
pub struct HarnessReport {
    /// The run, as it ended.
    pub run: Run,
    /// The signals, in order.
    pub signals: Vec<CuaSignal>,
    /// The log lines of taking the window, in order.
    pub notes: Vec<StepNote>,
}

/// Why a harness run did not end.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HarnessError {
    /// The run is waiting for something no fake will ever send.
    #[error("stuck in {0:?}")]
    Stuck(RunPhase),
    /// More steps than the script allows for.
    #[error("too many steps")]
    TooManySteps,
}

/// The most inputs a harness run takes before it gives up.
const MAX_INPUTS: usize = 2_000;

/// Drives `run` to its end over `world`: carry out each effect against the fake that owns it,
/// feed the answer back as a `RunInput`, advance the clock only when a fake says time passes.
///
/// `run` is a run as `Run::start` left it, in `Granting` (its first effect, the consent, is
/// carried out here); a finished one is returned as it is. Whatever the machine waits for
/// that no fake will send (the person's answer, a confirmation nobody shows) is `Stuck`.
pub async fn run_harness(run: Run, world: &mut World) -> Result<HarnessReport, HarnessError> {
    run_harness_answering(run, world, Vec::new()).await
}

/// [`run_harness`] with a scripted person: each question the run asks (the model's own, or one a
/// sensitive window forces) is answered with the next of `answers`, in order. Once they are
/// used up, a question is left unanswered and the run is `Stuck`.
pub async fn run_harness_answering(
    run: Run,
    world: &mut World,
    answers: Vec<String>,
) -> Result<HarnessReport, HarnessError> {
    let mut answers: VecDeque<String> = answers.into();
    let mut signals = Vec::new();
    let mut notes = Vec::new();
    if run.phase.is_finished() {
        return Ok(HarnessReport {
            run,
            signals,
            notes,
        });
    }
    let Parts {
        desktop,
        seat,
        capture,
        a11y,
        model,
        gate,
        events,
        clock,
        rules,
    } = world;
    let seam = Seam::<FakeSeams> {
        desktop,
        seat,
        capture,
        a11y,
        gate,
        events,
        clock,
        rules,
    };
    let mut exec = Exec::<FakeSeams>::new(seam, run.id.clone(), run.task.clone());
    let mut effects: VecDeque<RunEffect> = VecDeque::new();
    if run.phase == RunPhase::Granting {
        effects.push_back(RunEffect::Grant {
            app: run.task.app.clone(),
            space: run.task.space.clone(),
            level: run.env.grant_level,
        });
    }
    let mut run = run;
    let mut out = Collected::default();
    for _ in 0..MAX_INPUTS {
        while let Some(effect) = effects.pop_front() {
            exec.carry_out(effect, Some(&mut *model), &mut out).await;
        }
        for signal in &out.signals {
            if let CuaSignal::AskUser(id, _) = signal
                && let Some(answer) = answers.pop_front()
            {
                out.inputs.push_back(RunInput::UserAnswer(
                    *id,
                    prov::Labelled::new(answer, prov::Label::trusted_user()),
                ));
            }
        }
        signals.append(&mut out.signals);
        notes.append(&mut out.notes);
        let input = match out.inputs.pop_front() {
            Some(input) => input,
            None if run.phase.is_finished() => {
                return Ok(HarnessReport {
                    run,
                    signals,
                    notes,
                });
            }
            None => match idle(&run, &mut exec, clock).await {
                Some(input) => input,
                None => return Err(HarnessError::Stuck(run.phase)),
            },
        };
        let (next, fx) = run.step(input, clock.now());
        run = next;
        effects.extend(fx);
    }
    Err(HarnessError::TooManySteps)
}

/// What the world does when nothing is queued: a screen that changes, or time that passes (the
/// clock jumps to the machine's own wake-up, since nothing else will move it).
async fn idle(run: &Run, exec: &mut Exec<'_, FakeSeams>, clock: &FixedClock) -> Option<RunInput> {
    if run.track.settle.wants_damage()
        && let Some(damage) = exec.next_damage().await
    {
        return Some(RunInput::Damage(damage));
    }
    let wake = run.wake()?;
    let now = clock.now();
    if wake > now {
        clock.advance(wake.0 - now.0);
    }
    Some(RunInput::Tick)
}
