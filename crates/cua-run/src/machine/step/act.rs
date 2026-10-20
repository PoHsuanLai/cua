//! Carrying an action out: the input, its receipt, the damage after it, settling and the checks.

use super::proceed;
use crate::machine::effect::RunEffect;
use crate::machine::finish::{Said, finish, finish_plain};
use crate::machine::ops::{
    AlsoContext, action_ended, aim, drop_rest, enter, glow_for, next, observe, synthetic,
};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::{DamageWanted, SettleWatch, Waited};
use crate::observation::CursorObs;
use cua_action::{CuaAction, Point, WindowSpace};
use cua_bus::{
    ActionOutcome, ActionResult, CuaOutcome, FailReason, LocalRefusal, Refusal, RunBudgetKind,
    TargetedAction,
};
use cua_desktop::{Ms, Stamp};
use maskcap::{DamageAt, FrameWant, Settle, settle};
use porter_core::Count;
use porter_infer::StepIndex;
use seat_input::{InputReceipt, SeatError};

/// Counts the actions of the last minute and, if one more is allowed, injects the head.
pub(super) fn act(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, n: StepIndex) {
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

pub(super) fn injected(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, receipt: InputReceipt) {
    let RunPhase::Acting(n) = run.phase else {
        return;
    };
    run.track.acted.push(at);
    run.track.waited = Waited::No;
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
    // One input action per observation: whatever came next in the batch was chosen against the
    // screen as it was, and it is neither re-observed nor re-authorised until the model looks again.
    drop_rest(run);
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
pub(super) fn pointer_end(
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

pub(super) fn inject_failed(run: &mut Run, fx: &mut Vec<RunEffect>, error: SeatError) {
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
        SeatError::NotFocused => ActionResult::Refused(Refusal::Local(LocalRefusal::NotFocused)),
        other => ActionResult::Failed(other),
    };
    end_head(run, fx, n, head, result);
}

/// The window moved under the aim: the action was not injected, the model is told, and the run
/// looks again.
pub(super) fn state_moved(run: &mut Run, fx: &mut Vec<RunEffect>) {
    let RunPhase::Acting(n) = run.phase else {
        return;
    };
    let Some(head) = run.pending.actions.pop_front() else {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Input));
        return;
    };
    let result = ActionResult::Refused(Refusal::Local(LocalRefusal::StateMoved));
    end_head(run, fx, n, head, result);
}

/// The head action ended without running: the rest of the batch is dropped and the run looks.
fn end_head(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    n: StepIndex,
    head: TargetedAction,
    result: ActionResult,
) {
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

pub(super) fn damaged(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp, damage: DamageAt) {
    let newest = |latest: Option<DamageAt>| match latest {
        Some(old) if old.at >= damage.at => old,
        _ => damage,
    };
    match run.track.settle {
        SettleWatch::Damage { since, latest } => {
            run.track.settle = SettleWatch::Damage {
                since,
                latest: Some(newest(latest)),
            };
            settling(run, fx, at);
        }
        SettleWatch::Change { until, latest } => {
            run.track.settle = SettleWatch::Change {
                until,
                latest: Some(newest(latest)),
            };
            settling(run, fx, at);
        }
        SettleWatch::Idle => {}
    }
}

/// Where a wait stands at `at`: over at its deadline, or once the window has changed and been
/// quiet for `quiet`; otherwise it wakes at the sooner of the two.
fn wait_verdict(until: Stamp, latest: Option<DamageAt>, quiet: Ms, at: Stamp) -> Settle {
    if at >= until {
        return Settle::Quiet;
    }
    match latest {
        Some(change) => {
            let calm = Stamp(change.at.0.saturating_add(u64::from(quiet.0)));
            if at >= calm {
                Settle::Quiet
            } else {
                Settle::Waiting {
                    wake: calm.min(until),
                }
            }
        }
        None => Settle::Waiting { wake: until },
    }
}

/// Looks at the settle watch again at `at`: still waiting, or on to the next action.
pub(super) fn settling(run: &mut Run, fx: &mut Vec<RunEffect>, at: Stamp) {
    let RunPhase::Settling(n, _) = run.phase else {
        return;
    };
    let verdict = match run.track.settle {
        SettleWatch::Idle => return,
        SettleWatch::Change { until, latest } => {
            wait_verdict(until, latest, run.env.settle.quiet, at)
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
            let want = match run.track.settle {
                SettleWatch::Damage { .. } => FrameWant::AfterDamage,
                SettleWatch::Change {
                    latest: Some(_), ..
                } => FrameWant::AfterDamage,
                SettleWatch::Change { latest: None, .. } | SettleWatch::Idle => FrameWant::Now,
            };
            if run.track.settle.wants_damage() == DamageWanted::Yes {
                fx.push(RunEffect::CancelSettle);
            }
            run.track.settle = SettleWatch::Idle;
            proceed(run, fx, at, want);
        }
    }
}

pub(super) fn checked(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    result: crate::machine::input::CheckResult,
) {
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
