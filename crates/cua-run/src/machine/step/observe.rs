//! What the machine does with a look at the window: an observation, or a capture that failed.

use super::over_budget;
use super::screen;
use crate::attempts::Tried;
use crate::machine::effect::{HintSource, RunEffect};
use crate::machine::finish::{finish_plain, finish_stalled};
use crate::machine::ops::screen_label;
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::observation::{CursorObs, Observation};
use crate::progress::Look;
use crate::stall::{Repeats, Stall};
use cua_action::ActionClass;
use cua_bus::{ActionOutcome, ActionResult, FrameNote};
use cua_bus::{CuaOutcome, CuaRecord, FailReason, RunBudgetKind};
use maskcap::{FrameDigest, FrameWant, digest};
use porter_ids::Count;

/// Whether one of the actions ended as done and sent input.
fn input_ran(prev: &[ActionOutcome]) -> bool {
    prev.iter().any(|o| {
        o.result == ActionResult::Done
            && matches!(
                o.action.action.class(),
                ActionClass::Pointer | ActionClass::Keyboard
            )
    })
}

pub(super) fn observed(run: &mut Run, fx: &mut Vec<RunEffect>, mut obs: Observation) {
    let RunPhase::Observing(n) = run.phase else {
        return;
    };
    obs.step = n;
    obs.prev = std::mem::take(&mut run.track.outcomes);
    if obs.cursor == CursorObs::Unknown
        && let Some(before) = &run.working
    {
        obs.cursor = before.cursor;
    }
    let frame_digest: FrameDigest = digest(&obs.frame.pixels);
    run.frames = run.frames.seen(frame_digest);
    let look = Look::of(frame_digest, &obs.tree);
    // Only an input that ran says what it did: a look after a refusal or a model failure keeps
    // the effect of the input before it.
    if input_ran(&obs.prev)
        && let Some(before) = &run.track.look
    {
        run.track.effect = look.effect_since(before);
        fx.push(RunEffect::Record(Box::new(CuaRecord::Seen {
            run: run.id.clone(),
            n,
            effect: run.track.effect,
        })));
    }
    run.track.tried.extend(
        obs.prev
            .iter()
            .filter_map(|o| Tried::of(&o.action.action, &o.result, run.track.effect)),
    );
    run.track.look = Some(look);
    // A slow job (a render, an export) looks like a screen that stopped changing: while the
    // run has been waiting for no longer than its total, that is not a stall.
    let stall = match run.track.waited {
        Some(spent) if spent <= run.env.wait_total => {
            run.frames = Repeats::default().seen(frame_digest);
            Stall::Moving
        }
        Some(_) | None => run.frames.stall(run.task.budget.stall_frames),
    };
    run.used.stalled = run.frames.count();
    run.track.frame = Some(FrameNote::NotKept {
        digest: frame_digest,
        size: obs.frame.pixels.size,
    });
    run.taint = run.taint.join(&screen_label(run));
    run.track.thought = None;
    run.track.proposed = 0;
    run.used.steps = Count(n.0);
    if stall == Stall::Stalled {
        finish_stalled(run, fx);
        return;
    }
    if over_budget(run, RunBudgetKind::Steps) {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Steps));
        return;
    }
    run.working = Some(Box::new(obs));
    screen::stage(run, fx, n);
}

pub(super) fn capture_failed(run: &mut Run, fx: &mut Vec<RunEffect>, error: maskcap::CaptureError) {
    let RunPhase::Observing(_) = run.phase else {
        return;
    };
    use crate::machine::track::CaptureTries;
    match (error, run.track.capture) {
        (maskcap::CaptureError::SourceGone, _) => {
            finish_plain(run, fx, CuaOutcome::Failed(FailReason::WindowGone));
        }
        (_, CaptureTries::First) => {
            run.track.capture = CaptureTries::Retried;
            fx.push(RunEffect::Capture {
                want: FrameWant::Now,
                hints: HintSource::FromTree,
            });
        }
        (_, CaptureTries::Retried) => {
            finish_plain(run, fx, CuaOutcome::Failed(FailReason::Capture));
        }
    }
}
