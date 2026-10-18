//! Authorising the next action: the router's gate, the person's questions and the confirmation.

use super::act::act;
use super::screen;
use crate::machine::control::{self, pause};
use crate::machine::effect::{CuaSignal, RunEffect};
use crate::machine::finish::{Said, finish, finish_plain};
use crate::machine::ops::{
    AlsoContext, action_ended, aim, ask_for, drop_rest, enter, next, nodes_of, observe,
    screen_label, targeted,
};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::{Held, SettleWatch};
use crate::sensitivity::{FORCED_NO, FORCED_YES, WindowSensitivity, window_sensitivity};
use cua_action::{ActionClass, CuaAction, FinishOutcome};
use cua_bus::{
    ActionOutcome, ActionResult, AskBody, AuthorisedBy, BlockedOn, ConfirmBy, CuaOutcome,
    CuaRecord, FailReason, LocalRefusal, QuestionId, Refusal, SuccessCheck, TargetedAction,
};
use cua_desktop::{ConsentGuard, Ms, Stamp, Suspend};
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
    // Only the app the person allowed is driven: a window of another app is never aimed at,
    // whatever the lease said at the start.
    let aimed = if obs.window.app == run.task.app {
        aim(&obs, &head.action)
    } else {
        Err(crate::machine::ops::Unaimed::OutsideApp)
    };
    match aimed {
        Ok(_) => {
            run.track.authorised = AuthorisedBy::Policy;
            enter(run, fx, RunPhase::Authorising(n));
            fx.push(RunEffect::Check(Box::new(ask_for(run, &obs, &head.action))));
        }
        Err(why) => {
            run.pending.actions.pop_front();
            let result = match why {
                crate::machine::ops::Unaimed::Outside
                | crate::machine::ops::Unaimed::OutsideApp => {
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
    run.track.waited = run.track.waited.and(Ms(ms));
    action_ended(
        run,
        fx,
        ActionOutcome {
            action: head,
            result: ActionResult::Done,
        },
        None,
    );
    run.track.settle = SettleWatch::Change {
        until,
        latest: None,
    };
    fx.push(RunEffect::WatchSettle {
        quiet: run.env.settle.quiet,
        timeout: Ms(ms),
    });
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
    run.pending.actions.truncate(1);
    run.track.held = Held::Nothing;
    let choices: Vec<&str> = choices.iter().map(|c| c.as_str()).collect();
    raise(run, fx, n, question.as_str(), &choices);
}

/// Puts a question to the person and waits for the answer: the model's own, or the one a
/// sensitive window forces.
pub(super) fn raise(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    n: StepIndex,
    question: &str,
    choices: &[&str],
) {
    // The step's index, or the one after the last question when a step asks twice (a forced
    // question, then the model's own).
    let id = QuestionId(u64::from(n.0).max(run.track.last_question.saturating_add(1)));
    run.track.last_question = id.0;
    let label: Label = screen_label(run);
    let body = AskBody {
        question: Labelled::new(question.to_owned(), label.clone()),
        choices: choices
            .iter()
            .map(|c| Labelled::new((*c).to_owned(), label.clone()))
            .collect(),
    };
    fx.push(RunEffect::Emit(CuaSignal::AskUser(id, body.clone())));
    fx.push(RunEffect::Record(Box::new(CuaRecord::Asked {
        run: run.id.clone(),
        n,
        question_id: id,
        question: body.question,
    })));
    fx.push(RunEffect::Glow(cua_desktop::GlowState::WaitingForYou));
    control::progress(run, fx, question);
    enter(run, fx, RunPhase::WaitingForUser(n, id));
}

/// How sensitive the window is that the head action drives; none for an action that is not
/// input, and for an ordinary window.
fn sensitive_head(run: &Run) -> Option<WindowSensitivity> {
    let obs = run.working.as_ref()?;
    let head = run.pending.actions.front()?;
    let drives = matches!(
        head.action.class(),
        ActionClass::Pointer | ActionClass::Keyboard
    );
    let sensitivity = window_sensitivity(&obs.window.app, &obs.window.title.value, nodes_of(obs));
    (drives && sensitivity.asks()).then_some(sensitivity)
}

/// The router said run, and the window is sensitive: the person is asked first, as for any
/// question, and the head action stays queued until they answer.
fn hold(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, why: WindowSensitivity) {
    run.track.held = Held::Sensitive(why);
    let question = format!("The next step is in {}. Go ahead with it?", why.noun());
    raise(run, fx, n, &question, &[FORCED_YES, FORCED_NO]);
}

/// The person's answer to a held step: yes lets it run as a confirmed step; anything else
/// declines it.
fn held_answer(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    at: Stamp,
    n: StepIndex,
    answer: &Labelled<String>,
) {
    run.track.held = Held::Nothing;
    fx.push(RunEffect::Glow(cua_desktop::GlowState::Working));
    if answer.value == FORCED_YES {
        fx.push(RunEffect::Record(Box::new(CuaRecord::Confirmed {
            run: run.id.clone(),
            n,
            by: ConfirmBy::ShellPrompt,
        })));
        run.track.authorised = AuthorisedBy::User;
        act(run, fx, at, n);
    } else {
        end_head(run, fx, n, ActionResult::UserDeclined);
    }
}

/// The head action ends without running, the rest of the batch with it, and the run looks again.
fn end_head(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, result: ActionResult) {
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

pub(super) fn answered(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    at: Stamp,
    question: QuestionId,
    answer: Labelled<String>,
) {
    let RunPhase::WaitingForUser(n, asked) = run.phase.clone() else {
        return;
    };
    if question != asked {
        return;
    }
    match run.track.held {
        Held::Nothing => {}
        Held::SuspiciousScreen => {
            screen::answered(run, fx, n, &answer);
            return;
        }
        Held::Sensitive(_) | Held::AfterSuspicion => {
            held_answer(run, fx, at, n, &answer);
            return;
        }
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
            } else if let Some(why) = sensitive_head(run) {
                hold(run, fx, n, why);
                return;
            } else if let Some(effect) = screen::suspicious_head(run) {
                screen::hold_after_suspicion(run, fx, n, effect);
                return;
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
            end_head(run, fx, n, result);
        }
    }
}
