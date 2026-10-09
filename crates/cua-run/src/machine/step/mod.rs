//! The transition rows of the phase table (`cua.md` section 4, as the router's gate changed
//! it): `step` finds the row of an input in the run's phase. One module per kind of row.

mod act;
mod authorise;
mod model;
mod observe;

use act::{checked, damaged, inject_failed, injected, settling};
use authorise::{answered, confirming, verdict};
use model::proceed;
use model::{model_event, model_failed, model_replied};
use observe::{capture_failed, observed};

use crate::machine::control::{cancel, desktop, hand_back, pause, resume, take_over};
use crate::machine::effect::RunEffect;
use crate::machine::finish::finish_plain;
use crate::machine::input::RunInput;
use crate::machine::ops::{AlsoContext, count_active, enter, observe};
use crate::machine::phase::RunPhase;
use crate::machine::run::{Run, RunEnv};
use crate::machine::track::RunTrack;
use crate::mode::mode_support;
use crate::stall::Repeats;
use cua_bus::CuaTask;
use cua_bus::{BlockedOn, CuaOutcome, CuaRecord, ModeSupport, RunBudgetKind, TaskTarget};
use cua_desktop::{Stamp, ToplevelIdent};
use docket_core::GrantAnswer;
use maskcap::{FrameWant, Settle};
use porter_infer::StepIndex;
use prov::RunId;

/// The window a task names.
fn window_of(task: &CuaTask) -> Option<&ToplevelIdent> {
    match &task.target {
        TaskTarget::Window(ident) => Some(ident),
        TaskTarget::App(_) => None,
    }
}

/// A run, started: `Granting`, asking the router for the per-app consent; or, at once, over:
/// the mode is one the backend cannot run, or the task names an app and not a window.
pub(crate) fn start(id: RunId, task: CuaTask, env: RunEnv, at: Stamp) -> (Run, Vec<RunEffect>) {
    let taint = task.goal.label.clone();
    let mut run = Run {
        id,
        task,
        env,
        phase: RunPhase::Granting,
        used: cua_bus::BudgetUse::NONE,
        taint,
        frames: Repeats::default(),
        pending: crate::machine::phase::ActionQueue::default(),
        lease: None,
        served: None,
        working: None,
        last_at: at,
        track: RunTrack::new(),
    };
    let mut fx = Vec::new();
    let mode = run.task.mode;
    if let ModeSupport::Unsupported(_) = mode_support(&run.env.caps, mode) {
        finish_plain(
            &mut run,
            &mut fx,
            CuaOutcome::Blocked(BlockedOn::ModeUnsupported(mode)),
        );
    } else if window_of(&run.task).is_none() {
        finish_plain(
            &mut run,
            &mut fx,
            CuaOutcome::Blocked(BlockedOn::NeedsCapability),
        );
    } else {
        fx.push(RunEffect::Grant {
            app: run.task.app.clone(),
            space: run.task.space.clone(),
        });
    }
    (run, fx)
}

/// One transition.
pub(crate) fn advance(run: &mut Run, input: RunInput, at: Stamp) -> Vec<RunEffect> {
    let mut fx = Vec::new();
    if run.phase.is_finished() {
        return fx;
    }
    // A message is input and nothing more: it moves no clock either.
    if !matches!(input, RunInput::Inbound(_)) {
        count_active(run, at);
    }
    dispatch(run, &mut fx, input, at);
    fx
}

fn dispatch(run: &mut Run, fx: &mut Vec<RunEffect>, input: RunInput, at: Stamp) {
    match input {
        RunInput::UserCancel => cancel(run, fx),
        RunInput::Desktop(event) => desktop(run, fx, event),
        RunInput::UserPause => pause(run, fx),
        RunInput::UserResume => resume(run, fx),
        RunInput::UserTakeOver => take_over(run, fx, at),
        RunInput::UserHandBack => hand_back(run, fx),
        // Input like any other: it changes nothing here, and the supervisor hands what it says
        // to the model as a note at the next look.
        RunInput::Inbound(_) => {}
        RunInput::Routed(served) => {
            fx.push(RunEffect::Record(Box::new(CuaRecord::Routed {
                run: run.id.clone(),
                served_by: served.clone(),
            })));
            run.served = Some(served);
        }
        RunInput::Tick => tick(run, fx, at),
        RunInput::Granted(answer) => granted(run, fx, answer),
        RunInput::GrantFailed(_) => {
            finish_plain(run, fx, CuaOutcome::Blocked(BlockedOn::ConsentDenied))
        }
        RunInput::Leased(grant) => leased(run, fx, grant.id),
        RunInput::LeaseRefused(_) => {
            finish_plain(run, fx, CuaOutcome::Blocked(BlockedOn::LeaseRefused))
        }
        RunInput::Observed(obs) => observed(run, fx, *obs),
        RunInput::CaptureFailed(error) => capture_failed(run, fx, error),
        RunInput::ModelEvent(event) => model_event(run, fx, event),
        RunInput::ModelReplied(reply) => model_replied(run, fx, at, *reply),
        RunInput::ModelFailed(error) => model_failed(run, fx, error),
        RunInput::Confirming(id) => confirming(run, fx, id),
        RunInput::Verdict(answer) => verdict(run, fx, at, answer),
        RunInput::GateFailed(_) => {
            finish_plain(run, fx, CuaOutcome::Blocked(BlockedOn::PolicyDenials))
        }
        RunInput::Injected(receipt) => injected(run, fx, at, receipt),
        RunInput::InjectFailed(error) => inject_failed(run, fx, error),
        RunInput::Damage(damage) => damaged(run, fx, at, damage),
        RunInput::Checked(result) => checked(run, fx, result),
        RunInput::UserAnswer(question, answer) => answered(run, fx, question, answer),
    }
}

fn granted(run: &mut Run, fx: &mut Vec<RunEffect>, answer: GrantAnswer) {
    if run.phase != RunPhase::Granting {
        return;
    }
    match (answer, window_of(&run.task).cloned()) {
        (GrantAnswer::Granted, Some(window)) => {
            enter(run, fx, RunPhase::Leasing);
            fx.push(RunEffect::AcquireLease(cua_desktop::LeaseRequest {
                window,
                mode: run.task.mode,
                space: run.task.space.clone(),
                placement: cua_desktop::ReleasePlacement::ReturnToUser,
            }));
        }
        (GrantAnswer::Granted, None) => {
            finish_plain(run, fx, CuaOutcome::Blocked(BlockedOn::NeedsCapability));
        }
        (GrantAnswer::Refused(_), _) => {
            finish_plain(run, fx, CuaOutcome::Blocked(BlockedOn::ConsentDenied));
        }
    }
}

fn leased(run: &mut Run, fx: &mut Vec<RunEffect>, lease: cua_desktop::LeaseId) {
    if run.phase != RunPhase::Leasing {
        return;
    }
    run.lease = Some(lease);
    fx.push(RunEffect::Record(Box::new(CuaRecord::RunStarted {
        run: run.id.clone(),
        session: run.task.session.clone(),
        goal: run.task.goal.clone(),
        app: run.task.app.clone(),
        space: run.task.space.clone(),
        mode: run.task.mode,
        backend: run.env.caps.kind,
        budget: run.task.budget,
    })));
    fx.push(RunEffect::Glow(cua_desktop::GlowState::Working));
    observe(run, fx, StepIndex(1), FrameWant::Now, AlsoContext::Yes);
}

fn over_budget(run: &Run, kind: RunBudgetKind) -> bool {
    match kind {
        RunBudgetKind::Steps => run.used.steps > run.task.budget.steps,
        RunBudgetKind::Active => run.used.active >= run.task.budget.active,
        RunBudgetKind::Spend => run.used.spend > run.task.budget.spend,
        RunBudgetKind::Stall | RunBudgetKind::Rate => false,
    }
}

fn tick(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp) {
    if run.phase.is_active() && over_budget(run, RunBudgetKind::Active) {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Active));
        return;
    }
    settling(run, fx, at);
}

/// How long the machine may sleep before it wants a `Tick`: until a settle wake-up or until the
/// active budget runs out, whichever is first; none when it waits for the world.
pub(crate) fn wake(run: &Run) -> Option<Stamp> {
    let settle = match run.phase {
        RunPhase::Settling(_, Settle::Waiting { wake }) => Some(wake),
        _ => None,
    };
    let deadline = run.phase.is_active().then(|| {
        let left = run.task.budget.active.0.saturating_sub(run.used.active.0);
        Stamp(run.last_at.0.saturating_add(u64::from(left)))
    });
    match (settle, deadline) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}
