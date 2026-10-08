//! The transition rows of the phase table (`cua.md` section 4, as the router's gate changed
//! it): `step` finds the row of an input in the run's phase.

use crate::machine::control::{self, cancel, desktop, hand_back, pause, resume, take_over};
use crate::machine::effect::{CuaSignal, HintSource, RunEffect};
use crate::machine::finish::{Said, finish, finish_plain};
use crate::machine::input::RunInput;
use crate::machine::ops::{
    AlsoContext, action_ended, aim, ask_for, count_active, drop_rest, enter, glow_for, next,
    observe, screen_label, step_of, synthetic, targeted, thought_digest,
};
use crate::machine::phase::RunPhase;
use crate::machine::run::{Run, RunEnv};
use crate::machine::track::{RunTrack, SettleWatch};
use crate::mode::mode_support;
use crate::observation::{CursorObs, Observation};
use crate::stall::{Repeats, Stall};
use crate::turn::turn;
use cua_action::{ActionClass, CuaAction, FinishOutcome, Point, WindowSpace};
use cua_bus::{
    ActionOutcome, ActionResult, AskBody, AuthorisedBy, BlockedOn, ConfirmBy, CuaOutcome,
    CuaRecord, FailReason, LocalRefusal, ModeSupport, QuestionId, Refusal, RunBudgetKind,
    SuccessCheck, TargetedAction, TaskTarget,
};
use cua_bus::{CuaTask, FrameNote};
use cua_desktop::{ConsentGuard, Stamp, Suspend, ToplevelIdent};
use docket_core::{CallRefusal, GateAnswer, GrantAnswer};
use maskcap::{DamageAt, FrameDigest, FrameWant, Settle, digest, settle};
use porter_core::Count;
use porter_infer::{CuaStepReply, StepIndex};
use prov::{Label, Labelled, RunId};
use seat_input::{InputReceipt, SeatError};

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

fn observed(run: &mut Run, fx: &mut Vec<RunEffect>, mut obs: Observation) {
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
    run.used.stalled = run.frames.count();
    run.track.frame = Some(FrameNote::NotKept {
        digest: frame_digest,
        size: obs.frame.pixels.size,
    });
    run.taint = run.taint.join(&screen_label(run));
    run.track.thought = None;
    run.track.proposed = 0;
    run.used.steps = Count(n.0);
    if run.frames.stall(run.task.budget.stall_frames) == Stall::Stalled {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Stall));
        return;
    }
    if over_budget(run, RunBudgetKind::Steps) {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Steps));
        return;
    }
    let (request, frame) = turn(&obs);
    run.working = Some(Box::new(obs));
    enter(run, fx, RunPhase::Thinking(n));
    fx.push(RunEffect::CallModel {
        request: Box::new(request),
        frame,
    });
}

fn capture_failed(run: &mut Run, fx: &mut Vec<RunEffect>, error: maskcap::CaptureError) {
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

fn model_event(run: &mut Run, fx: &mut Vec<RunEffect>, event: crate::model::StepEvent) {
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
fn model_failure(run: &mut Run, fx: &mut Vec<RunEffect>, again: StepIndex) {
    run.used.model_failures = Count(run.used.model_failures.0.saturating_add(1));
    if run.used.model_failures > run.task.budget.model_failures {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Model));
    } else {
        run.pending.actions.clear();
        observe(run, fx, again, FrameWant::Now, AlsoContext::No);
    }
}

fn model_failed(run: &mut Run, fx: &mut Vec<RunEffect>, error: crate::model::CuaModelError) {
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

fn model_replied(
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
pub(crate) fn proceed(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, want: FrameWant) {
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

fn authorise(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, head: TargetedAction) {
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

fn wait(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, n: StepIndex, asked: u32) {
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

fn conclude(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex) {
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

fn ask(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, head: TargetedAction) {
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

fn answered(
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

fn confirming(run: &mut Run, fx: &mut Vec<RunEffect>, id: prov::ConfirmId) {
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
fn resume_after_confirmation(run: &Run, fx: &mut Vec<RunEffect>) {
    if run.env.caps.consent == ConsentGuard::CuadSuspendsInput
        && let Some(lease) = run.lease
    {
        fx.push(RunEffect::ResumeLease(lease));
    }
}

fn verdict(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, answer: GateAnswer) {
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

/// Counts the actions of the last minute and, if one more is allowed, injects the head.
fn act(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, n: StepIndex) {
    const MINUTE: u64 = 60_000;
    run.track
        .acted
        .retain(|t| at.0.saturating_sub(t.0) < MINUTE);
    let in_minute = u32::try_from(run.track.acted.len()).unwrap_or(u32::MAX);
    run.used.actions_last_minute = Count(in_minute);
    if in_minute >= run.task.budget.actions_per_min.0 {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Rate));
        return;
    }
    let (Some(head), Some(obs)) = (run.pending.actions.front().cloned(), run.working.clone())
    else {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Input));
        return;
    };
    match aim(&obs, &head.action) {
        Ok(planned) => {
            enter(run, fx, RunPhase::Acting(n));
            fx.push(RunEffect::Glow(glow_for(&planned)));
            fx.push(RunEffect::Inject(planned));
        }
        Err(_) => {
            // The tree changed under a confirmation: the point is no longer there.
            run.pending.actions.pop_front();
            let outcome = ActionOutcome {
                action: head,
                result: ActionResult::Refused(Refusal::Local(LocalRefusal::OutsideLease)),
            };
            action_ended(run, fx, outcome, None);
            drop_rest(run);
            observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
        }
    }
}

fn injected(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, receipt: InputReceipt) {
    let RunPhase::Acting(n) = run.phase else {
        return;
    };
    run.track.acted.push(at);
    let Some(head) = run.pending.actions.pop_front() else {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Input));
        return;
    };
    if let (Some(obs), Some(point)) = (run.working.as_mut(), pointer_end(&head.action, &receipt)) {
        obs.cursor = CursorObs::At(point);
    }
    action_ended(
        run,
        fx,
        ActionOutcome {
            action: head,
            result: ActionResult::Done,
        },
        Some(receipt),
    );
    let (quiet, timeout) = (run.env.settle.quiet, run.env.settle.timeout);
    run.track.settle = SettleWatch::Damage {
        since: at,
        latest: None,
    };
    fx.push(RunEffect::WatchSettle { quiet, timeout });
    enter(
        run,
        fx,
        RunPhase::Settling(
            n,
            Settle::Waiting {
                wake: Stamp(at.0.saturating_add(u64::from(quiet.0))),
            },
        ),
    );
}

/// Where the pointer ends up after an action, as far as the action says.
fn pointer_end(
    action: &CuaAction<WindowSpace>,
    _receipt: &InputReceipt,
) -> Option<Point<WindowSpace>> {
    match action {
        CuaAction::Click { at, .. } | CuaAction::MoveTo { at } | CuaAction::Scroll { at, .. } => {
            match at {
                cua_action::Target::Point(p) => Some(*p),
                _ => None,
            }
        }
        CuaAction::Drag {
            to: cua_action::Target::Point(p),
            ..
        } => Some(*p),
        _ => None,
    }
}

fn inject_failed(run: &mut Run, fx: &mut Vec<RunEffect>, error: SeatError) {
    let RunPhase::Acting(n) = run.phase else {
        return;
    };
    if error == SeatError::TargetGone {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::WindowGone));
        return;
    }
    let Some(head) = run.pending.actions.pop_front() else {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Input));
        return;
    };
    let result = match error {
        SeatError::OutsideLease => {
            ActionResult::Refused(Refusal::Local(LocalRefusal::OutsideLease))
        }
        other => ActionResult::Failed(other),
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

fn damaged(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, damage: DamageAt) {
    if let SettleWatch::Damage { since, latest } = run.track.settle {
        let newest = match latest {
            Some(old) if old.at >= damage.at => old,
            _ => damage,
        };
        run.track.settle = SettleWatch::Damage {
            since,
            latest: Some(newest),
        };
        settling(run, fx, at);
    }
}

fn tick(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp) {
    if run.phase.is_active() && over_budget(run, RunBudgetKind::Active) {
        finish_plain(run, fx, CuaOutcome::BudgetOut(RunBudgetKind::Active));
        return;
    }
    settling(run, fx, at);
}

/// Looks at the settle watch again at `at`: still waiting, or on to the next action.
fn settling(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp) {
    let RunPhase::Settling(n, _) = run.phase else {
        return;
    };
    let verdict = match run.track.settle {
        SettleWatch::Idle => return,
        SettleWatch::Timer { until } => {
            if at >= until {
                Settle::Quiet
            } else {
                Settle::Waiting { wake: until }
            }
        }
        SettleWatch::Damage { since, latest } => settle(
            latest.as_slice(),
            run.env.settle.quiet,
            run.env.settle.timeout,
            since,
            at,
        ),
    };
    match verdict {
        Settle::Waiting { wake } => enter(run, fx, RunPhase::Settling(n, Settle::Waiting { wake })),
        Settle::Quiet | Settle::TimedOut => {
            let was_damage = matches!(run.track.settle, SettleWatch::Damage { .. });
            run.track.settle = SettleWatch::Idle;
            if was_damage {
                fx.push(RunEffect::CancelSettle);
            }
            let want = if was_damage {
                FrameWant::AfterDamage
            } else {
                FrameWant::Now
            };
            proceed(run, fx, at, want);
        }
    }
}

fn checked(run: &mut Run, fx: &mut Vec<RunEffect>, result: crate::machine::input::CheckResult) {
    let RunPhase::Verifying(n) = run.phase else {
        return;
    };
    let head = run.pending.actions.front().cloned();
    match result {
        crate::machine::input::CheckResult::Pass => {
            let said = match head.map(|h| h.action) {
                Some(CuaAction::Finish {
                    summary, extracted, ..
                }) => Said { summary, extracted },
                _ => Said::of(CuaOutcome::Done),
            };
            finish(run, fx, CuaOutcome::Done, said);
        }
        crate::machine::input::CheckResult::Fail => {
            run.pending.actions.clear();
            let outcome = synthetic(run, ActionResult::CheckFailed);
            run.track.outcomes.push(outcome);
            observe(run, fx, next(n), FrameWant::Now, AlsoContext::No);
        }
    }
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
