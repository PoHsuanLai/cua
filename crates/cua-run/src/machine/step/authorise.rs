//! Authorising the next action: the router's gate, the person's questions and the confirmation.

use super::act::act;
use crate::machine::control::{self, pause};
use crate::machine::effect::{CuaSignal, RunEffect};
use crate::machine::finish::{Said, finish, finish_plain};
use crate::machine::ops::{
    AlsoContext, action_ended, aim, ask_for, drop_rest, enter, next, observe, screen_label,
    targeted,
};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::SettleWatch;
use cua_action::{CuaAction, FinishOutcome};
use cua_bus::{
    ActionOutcome, ActionResult, AskBody, AuthorisedBy, BlockedOn, ConfirmBy, CuaOutcome,
    CuaRecord, FailReason, LocalRefusal, QuestionId, Refusal, SuccessCheck, TargetedAction,
};
use cua_desktop::{ConsentGuard, Stamp, Suspend};
use docket_core::{CallRefusal, GateAnswer};
use maskcap::{FrameWant, Settle};
use porter_infer::StepIndex;
use prov::{Label, Labelled};
use seat_input::SeatError;

pub(super) fn authorise(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    n: StepIndex,
    head: TargetedAction,
) {
    let Some(obs) = run.working.clone() else {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Capture));
        return;
    };
    match aim(&obs, &head.action) {
        Ok(_) => {
            run.track.authorised = AuthorisedBy::Policy;
            enter(run, fx, RunPhase::Authorising(n));
            fx.push(RunEffect::Check(Box::new(ask_for(run, &obs, &head.action))));
        }
        Err(why) => {
            run.pending.actions.pop_front();
            let result = match why {
                crate::machine::ops::Unaimed::Outside => {
                    ActionResult::Refused(Refusal::Local(LocalRefusal::OutsideLease))
                }
                crate::machine::ops::Unaimed::NoSuchNode => {
                    ActionResult::Failed(SeatError::Refused)
                }
            };
            action_ended(
                run,
                fx,
                ActionOutcome {
                    action: head,
                    result,
                },
                None,
            );
            drop_rest(run);
            observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
        }
    }
}

pub(super) fn wait(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, n: StepIndex, asked: u32) {
    let Some(head) = run.pending.actions.pop_front() else {
        observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
        return;
    };
    let ms = asked.min(run.env.wait_max.0);
    let until = Stamp(at.0.saturating_add(u64::from(ms)));
    action_ended(
        run,
        fx,
        ActionOutcome {
            action: head,
            result: ActionResult::Done,
        },
        None,
    );
    run.track.settle = SettleWatch::Timer { until };
    enter(
        run,
        fx,
        RunPhase::Settling(n, Settle::Waiting { wake: until }),
    );
}

pub(super) fn conclude(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex) {
    let Some(TargetedAction {
        action:
            CuaAction::Finish {
                outcome,
                summary,
                extracted,
            },
        ..
    }) = run.pending.actions.front().cloned()
    else {
        return;
    };
    let said = Said { summary, extracted };
    match (outcome, run.task.success.clone()) {
        (FinishOutcome::Failed | FinishOutcome::Infeasible, _) => {
            finish(run, fx, CuaOutcome::Failed(FailReason::Infeasible), said);
        }
        (FinishOutcome::Done, SuccessCheck::ModelSays) => finish(run, fx, CuaOutcome::Done, said),
        (FinishOutcome::Done, SuccessCheck::TreeHas(selector) | SuccessCheck::Both(selector)) => {
            enter(run, fx, RunPhase::Verifying(n));
            fx.push(RunEffect::VerifyTree(selector));
        }
    }
}

pub(super) fn ask(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, head: TargetedAction) {
    let CuaAction::Ask { question, choices } = &head.action else {
        return;
    };
    let id = QuestionId(u64::from(n.0));
    let label: Label = screen_label(run);
    let body = AskBody {
        question: Labelled::new(question.as_str().to_owned(), label.clone()),
        choices: choices
            .iter()
            .map(|c| Labelled::new(c.as_str().to_owned(), label.clone()))
            .collect(),
    };
    run.pending.actions.truncate(1);
    fx.push(RunEffect::Emit(CuaSignal::AskUser(id, body.clone())));
    fx.push(RunEffect::Record(Box::new(CuaRecord::Asked {
        run: run.id.clone(),
        n,
        question_id: id,
        question: body.question,
    })));
    fx.push(RunEffect::Glow(cua_desktop::GlowState::WaitingForYou));
    control::progress(run, fx, question.as_str());
    enter(run, fx, RunPhase::WaitingForUser(n, id));
}

pub(super) fn answered(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    question: QuestionId,
    answer: Labelled<String>,
) {
    let RunPhase::WaitingForUser(n, asked) = run.phase.clone() else {
        return;
    };
    if question != asked {
        return;
    }
    let action = run
        .pending
        .actions
        .pop_back()
        .unwrap_or_else(|| targeted(run, CuaAction::Observe));
    run.pending.actions.clear();
    let outcome = ActionOutcome {
        action,
        result: ActionResult::Answered(answer),
    };
    action_ended(run, fx, outcome, None);
    fx.push(RunEffect::Glow(cua_desktop::GlowState::Working));
    observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
}

pub(super) fn confirming(run: &mut Run, fx: &mut Vec<RunEffect>, id: prov::ConfirmId) {
    let RunPhase::Authorising(n) = run.phase else {
        return;
    };
    if run.env.caps.consent == ConsentGuard::CuadSuspendsInput
        && let Some(lease) = run.lease
    {
        fx.push(RunEffect::SuspendLease(lease, Suspend::Consent));
    }
    fx.push(RunEffect::Proceed);
    fx.push(RunEffect::Glow(cua_desktop::GlowState::WaitingForYou));
    enter(run, fx, RunPhase::Confirming(n, id));
}

/// Input flows again after a confirmation, on the backend that stopped it.
pub(super) fn resume_after_confirmation(run: &Run, fx: &mut Vec<RunEffect>) {
    if run.env.caps.consent == ConsentGuard::CuadSuspendsInput
        && let Some(lease) = run.lease
    {
        fx.push(RunEffect::ResumeLease(lease));
    }
}

pub(super) fn verdict(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, answer: GateAnswer) {
    let (n, confirmed) = match run.phase {
        RunPhase::Authorising(n) => (n, false),
        RunPhase::Confirming(n, _) => (n, true),
        _ => return,
    };
    if confirmed {
        resume_after_confirmation(run, fx);
    }
    match answer {
        GateAnswer::Run => {
            if confirmed {
                let by = match run.env.caps.consent {
                    ConsentGuard::CuadSuspendsInput => ConfirmBy::ShellPrompt,
                    ConsentGuard::CompositorOrigin => ConfirmBy::PhysicalInput,
                };
                fx.push(RunEffect::Record(Box::new(CuaRecord::Confirmed {
                    run: run.id.clone(),
                    n,
                    by,
                })));
                run.track.authorised = AuthorisedBy::User;
            }
            act(run, fx, at, n);
        }
        GateAnswer::Refused(CallRefusal::Paused(_)) => {
            finish_plain(run, fx, CuaOutcome::Blocked(BlockedOn::PolicyDenials));
        }
        GateAnswer::Refused(CallRefusal::Halted(_)) => pause(run, fx),
        GateAnswer::Refused(refusal) => {
            let result = match refusal {
                CallRefusal::Unconfirmed(_) => ActionResult::UserDeclined,
                other => ActionResult::Refused(Refusal::Gate(other)),
            };
            if let Some(head) = run.pending.actions.pop_front() {
                action_ended(
                    run,
                    fx,
                    ActionOutcome {
                        action: head,
                        result,
                    },
                    None,
                );
            }
            drop_rest(run);
            observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
        }
    }
}
