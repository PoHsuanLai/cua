//! What the person, the shell and the desktop do to a run from outside its loop: pause, resume,
//! take over, hand back, cancel, and what the desktop says on its own.

use crate::config::Autopause;
use crate::machine::effect::RunEffect;
use crate::machine::finish::finish_plain;
use crate::machine::ops::{AlsoContext, drop_rest, enter, next, observe, step_of, synthetic};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::SettleWatch;
use crate::mode::mode_support;
use cua_action::Summary;
use cua_bus::TaskTarget;
use cua_bus::{
    ActionResult, CancelledBy, CuaOutcome, CuaRecord, FailReason, ModeSupport, report_draft,
};
use cua_desktop::{DesktopEvent, GlowState, RevokeReason, Stamp, Suspend};
use maskcap::FrameWant;
use prov::ReportStatus;

/// Whether the person may pause in this phase: any that has a lease and is not already stopped.
fn pausable(phase: &RunPhase) -> bool {
    matches!(
        phase,
        RunPhase::Observing(_)
            | RunPhase::Thinking(_)
            | RunPhase::Authorising(_)
            | RunPhase::Confirming(..)
            | RunPhase::Acting(_)
            | RunPhase::Settling(..)
            | RunPhase::Verifying(_)
            | RunPhase::WaitingForUser(..)
    )
}

/// Stops what the phase was waiting on.
fn stop_waiting(run: &Run, fx: &mut Vec<RunEffect>) {
    match run.phase {
        RunPhase::Thinking(_) => fx.push(RunEffect::CancelModel),
        RunPhase::Settling(..) => fx.push(RunEffect::CancelSettle),
        _ => {}
    }
}

/// A progress report to the requester: the run waits for the person.
pub(crate) fn progress(run: &Run, fx: &mut Vec<RunEffect>, text: &str) {
    let said = Summary::new(text)
        .or_else(|_| Summary::new(""))
        .expect("the empty summary is valid");
    fx.push(RunEffect::Report(Box::new(report_draft(
        &run.task.report,
        ReportStatus::Progress,
        &said,
        &[],
    ))));
}

/// The person (or a hands-off rule) pauses the run: the model call and the settle watch stop,
/// the input stops, the batch is dropped, and the requester hears that it waits.
pub(crate) fn pause(run: &mut Run, fx: &mut Vec<RunEffect>) {
    if !pausable(&run.phase) {
        return;
    }
    let n = step_of(run);
    stop_waiting(run, fx);
    if let Some(lease) = run.lease {
        fx.push(RunEffect::SuspendLease(lease, Suspend::Paused));
    }
    fx.push(RunEffect::Glow(GlowState::WaitingForYou));
    drop_rest(run);
    run.track.settle = SettleWatch::Idle;
    enter(run, fx, RunPhase::Paused(n));
    progress(run, fx, "Paused.");
}

/// A paused run looks at the window afresh.
pub(crate) fn resume(run: &mut Run, fx: &mut Vec<RunEffect>) {
    let RunPhase::Paused(n) = run.phase else {
        return;
    };
    if let Some(lease) = run.lease {
        fx.push(RunEffect::ResumeLease(lease));
    }
    fx.push(RunEffect::Glow(GlowState::Working));
    observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
}

/// The person takes the window: input stops and the glow goes out.
pub(crate) fn take_over(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp) {
    let Some(lease) = run.lease else {
        return;
    };
    if matches!(run.phase, RunPhase::TakenOver(..) | RunPhase::Finished(_)) {
        return;
    }
    let n = step_of(run);
    stop_waiting(run, fx);
    fx.push(RunEffect::SuspendLease(lease, Suspend::TakeOver));
    fx.push(RunEffect::Glow(GlowState::Off));
    fx.push(RunEffect::Record(Box::new(CuaRecord::TakenOver {
        run: run.id.clone(),
        n,
    })));
    drop_rest(run);
    run.track.settle = SettleWatch::Idle;
    enter(run, fx, RunPhase::TakenOver(n, at));
}

/// The person hands the window back: the run learns that they acted, and looks afresh.
pub(crate) fn hand_back(run: &mut Run, fx: &mut Vec<RunEffect>) {
    let RunPhase::TakenOver(n, since) = run.phase else {
        return;
    };
    if let Some(lease) = run.lease {
        fx.push(RunEffect::ResumeLease(lease));
    }
    fx.push(RunEffect::Glow(GlowState::Working));
    fx.push(RunEffect::Record(Box::new(CuaRecord::HandedBack {
        run: run.id.clone(),
        n,
        user_events: Vec::new(),
    })));
    let outcome = synthetic(run, ActionResult::UserActed { since });
    run.track.outcomes.push(outcome);
    observe(run, fx, next(n), FrameWant::Now, AlsoContext::Yes);
}

/// The person cancels.
pub(crate) fn cancel(run: &mut Run, fx: &mut Vec<RunEffect>) {
    finish_plain(run, fx, CuaOutcome::Cancelled(CancelledBy::User));
}

fn window_ident(run: &Run) -> Option<&cua_desktop::ToplevelIdent> {
    match &run.task.target {
        TaskTarget::Window(ident) => Some(ident),
        TaskTarget::App(_) => None,
    }
}

/// What the desktop says on its own: the kill chord ends every run, a revoked lease or a closed
/// window ends this one, and physical input on a shared seat pauses a run that asked to be
/// paused by it.
pub(crate) fn desktop(run: &mut Run, fx: &mut Vec<RunEffect>, event: DesktopEvent) {
    match event {
        DesktopEvent::KillChord => {
            finish_plain(run, fx, CuaOutcome::Cancelled(CancelledBy::KillSwitch));
        }
        DesktopEvent::LeaseRevoked { lease, why } if Some(lease) == run.lease => {
            let outcome = match why {
                RevokeReason::KillChord => CuaOutcome::Cancelled(CancelledBy::KillSwitch),
                RevokeReason::WindowClosed
                | RevokeReason::SpaceChanged
                | RevokeReason::Compositor => CuaOutcome::Failed(FailReason::WindowGone),
            };
            // The lease is already gone: nothing to release.
            run.lease = None;
            finish_plain(run, fx, outcome);
        }
        DesktopEvent::WindowClosed(ident) if Some(&ident) == window_ident(run) => {
            finish_plain(run, fx, CuaOutcome::Failed(FailReason::WindowGone));
        }
        DesktopEvent::PhysicalInput { lease } if Some(lease) == run.lease => {
            let hands_off = mode_support(&run.env.caps, run.task.mode) == ModeSupport::HandsOff;
            if hands_off && run.env.autopause == Autopause::On {
                pause(run, fx);
            }
        }
        DesktopEvent::LeaseRevoked { .. }
        | DesktopEvent::WindowClosed(_)
        | DesktopEvent::PhysicalInput { .. }
        | DesktopEvent::GeometryChanged(..)
        | DesktopEvent::ConsentShown
        | DesktopEvent::ConsentHidden => {}
    }
}
