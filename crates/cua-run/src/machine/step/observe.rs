//! What the machine does with a look at the window: an observation, or a capture that failed.

use super::over_budget;
use super::screen;
use crate::machine::effect::{HintSource, RunEffect};
use crate::machine::finish::finish_plain;
use crate::machine::ops::screen_label;
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::Waited;
use crate::observation::{CursorObs, Observation};
use crate::stall::{Repeats, Stall};
use cua_bus::FrameNote;
use cua_bus::{CuaOutcome, FailReason, RunBudgetKind};
use maskcap::{FrameDigest, FrameWant, digest};
use porter_core::Count;

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
    // A slow job (a render, an export) looks like a screen that stopped changing: while the
    // run has been waiting for no longer than its total, that is not a stall.
    let stall = match run.track.waited {
        Waited::For(spent) if spent <= run.env.wait_total => {
            run.frames = Repeats::default().seen(frame_digest);
            Stall::Moving
        }
        Waited::For(_) | Waited::No => run.frames.stall(run.task.budget.stall_frames),
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
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Stall));
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
