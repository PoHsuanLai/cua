//! What the machine does with the model: its events, its reply and its failures.

use super::authorise::{ask, authorise, conclude, wait};
use super::over_budget;
use crate::machine::effect::{CuaSignal, RunEffect};
use crate::machine::finish::finish_plain;
use crate::machine::ops::{
    AlsoContext, action_ended, drop_rest, next, observe, step_of, targeted, thought_digest,
};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use cua_action::{ActionClass, CuaAction};
use cua_bus::{ActionOutcome, ActionResult, CuaOutcome, FailReason, RunBudgetKind};
use cua_desktop::Stamp;
use maskcap::FrameWant;
use porter_core::Count;
use porter_infer::{CuaStepReply, StepIndex};

pub(super) fn model_event(run: &mut Run, fx: &mut Vec<RunEffect>, event: crate::model::StepEvent) {
    let RunPhase::Thinking(n) = run.phase else {
        return;
    };
    match event {
        crate::model::StepEvent::ThoughtDelta(text) => {
            fx.push(RunEffect::Emit(CuaSignal::ThoughtDelta(n, text)));
        }
        crate::model::StepEvent::ActionProposed(action) => {
            run.track.proposed = run.track.proposed.saturating_add(1);
            fx.push(RunEffect::Emit(CuaSignal::ActionProposed(
                n,
                targeted(run, action),
            )));
        }
    }
}

/// A model failure that is not the end: counted, and the run looks again, at step `again`.
pub(super) fn model_failure(run: &mut Run, fx: &mut Vec<RunEffect>, again: StepIndex) {
    run.used.model_failures = Count(run.used.model_failures.0.saturating_add(1));
    if run.used.model_failures > run.task.budget.model_failures {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Model));
    } else {
        run.pending.actions.clear();
        observe(run, fx, again, FrameWant::Now, AlsoContext::No);
    }
}

pub(super) fn model_failed(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    error: crate::model::CuaModelError,
) {
    let RunPhase::Thinking(n) = run.phase else {
        return;
    };
    match error {
        crate::model::CuaModelError::Transient | crate::model::CuaModelError::Parse => {
            model_failure(run, fx, n);
        }
        crate::model::CuaModelError::Fatal => {
            finish_plain(run, fx, CuaOutcome::Failed(FailReason::Model));
        }
    }
}

pub(super) fn model_replied(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    at: Stamp,
    reply: crate::machine::input::ModelReply,
) {
    let RunPhase::Thinking(n) = run.phase else {
        return;
    };
    run.used.spend = porter_core::MicroUsd(run.used.spend.0.saturating_add(reply.cost.0));
    if over_budget(run, RunBudgetKind::Spend) {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Spend));
        return;
    }
    let CuaStepReply {
        thought,
        actions,
        safety,
        ..
    } = reply.reply;
    run.track.thought = thought.as_deref().map(thought_digest);
    if actions.is_empty() || !safety.is_empty() {
        // Nothing to do is a failure to answer; a vendor's block or ask is read as a block (the
        // router has no way to be told "confirm this", so the run does not go on without it).
        model_failure(run, fx, next(n));
        return;
    }
    let already = usize::try_from(run.track.proposed).unwrap_or(usize::MAX);
    for action in actions.iter().skip(already) {
        fx.push(RunEffect::Emit(CuaSignal::ActionProposed(
            n,
            targeted(run, action.clone()),
        )));
    }
    run.pending.actions = actions.into_iter().map(|a| targeted(run, a)).collect();
    proceed(run, fx, at, FrameWant::Now);
}

/// The head of the batch decides what comes next.
pub(super) fn proceed(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, want: FrameWant) {
    let n = step_of(run);
    let Some(head) = run.pending.actions.front().cloned() else {
        observe(run, fx, next(n), want, AlsoContext::No);
        return;
    };
    match head.action.class() {
        ActionClass::Pointer | ActionClass::Keyboard => authorise(run, fx, n, head),
        ActionClass::Observe => match head.action {
            CuaAction::Wait { for_ms } => wait(run, fx, at, n, for_ms.ms()),
            _ => {
                run.pending.actions.pop_front();
                let done = ActionOutcome {
                    action: head,
                    result: ActionResult::Done,
                };
                action_ended(run, fx, done, None);
                drop_rest(run);
                observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
            }
        },
        ActionClass::Conclude => match &head.action {
            CuaAction::Finish { .. } => conclude(run, fx, n),
            _ => ask(run, fx, n, head),
        },
    }
}
