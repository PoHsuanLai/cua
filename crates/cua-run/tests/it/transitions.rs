//! The phase table, one row at a time: the next phase and the effects that matter. The
//! rows follow `cua.md` section 4 as the router's gate changed it.

use crate::support::*;
use cua_action::{
    Choice, Coord, CuaAction, Extracted, ExtractedText, FieldName, FinishOutcome, Rect, Size,
    Summary,
};
use cua_bus::{
    ActionOutcome, ActionResult, BlockedOn, CuaOutcome, CuaRecord, FailReason, LocalRefusal,
    QuestionId, Refusal, RunBudgetKind, SuccessCheck,
};
use cua_desktop::{BackendKind, GlowState, LeaseId, Stamp, Suspend, ToplevelIdent};
use cua_run::{
    CheckResult, CuaModelError, CuaSignal, RunEffect, RunInput, RunPhase, SettleWatch, StepEvent,
};
use docket_core::{BreakerTrip, CallRefusal, ConfirmEnd, DenyCode, GateAnswer, GrantAnswer};
use maskcap::{CaptureError, DamageAt, DamageSource, FrameWant, Settle};
use porter_infer::StepIndex;
use prov::{ConfirmId, Labelled, MessageKind, ReportStatus, SpaceScope};
use seat_input::{InputOrigin, InputReceipt, SeatError};

const STOCK: BackendKind = BackendKind::KwinStock;

fn model_says() -> SuccessCheck {
    SuccessCheck::ModelSays
}

fn receipt(at: u64) -> InputReceipt {
    InputReceipt {
        lease: LeaseId(1),
        ops: porter_core::Count(3),
        text_path: None,
        origin: InputOrigin::SharedSeatViaEi,
        at: Stamp(at),
    }
}

fn damage(at: u64) -> DamageAt {
    DamageAt {
        at: Stamp(at),
        region: Rect {
            origin: cua_action::Point::new(Coord(0), Coord(0)),
            size: Size::new(Coord(1), Coord(1)),
        },
        source: DamageSource::PixelDiff,
    }
}

fn report_of(effects: &[RunEffect]) -> Option<(ReportStatus, Vec<String>)> {
    effects.iter().find_map(|e| match e {
        RunEffect::Report(draft) => match draft.kind {
            MessageKind::Report { status } => Some((
                status,
                draft
                    .parts
                    .iter()
                    .map(|p| match p {
                        docket_core::DraftPart::Text(t) => t.as_str().to_owned(),
                        other => panic!("{other:?}"),
                    })
                    .collect(),
            )),
            _ => None,
        },
        _ => None,
    })
}

fn kinds(effects: &[RunEffect]) -> Vec<String> {
    effects
        .iter()
        .filter_map(|e| match e {
            RunEffect::Record(r) => Some(r.kind()),
            _ => None,
        })
        .collect()
}

fn states(effects: &[RunEffect]) -> Vec<cua_bus::RunStateSlug> {
    effects
        .iter()
        .filter_map(|e| match e {
            RunEffect::Emit(CuaSignal::StateChanged(s)) => Some(*s),
            _ => None,
        })
        .collect()
}

fn finished_with(d: &Drive) -> Option<CuaOutcome> {
    match d.phase() {
        RunPhase::Finished(o) => Some(*o),
        _ => None,
    }
}

#[test]
fn the_consent_row_leases_the_window_on_yes_and_blocks_on_no() {
    let mut d = Drive::start(STOCK, task(model_says()));
    d.feed(RunInput::Granted(GrantAnswer::Granted));
    assert_eq!(*d.phase(), RunPhase::Leasing);
    let lease = d.effects.iter().find_map(|e| match e {
        RunEffect::AcquireLease(req) => Some(req.clone()),
        _ => None,
    });
    let lease = lease.expect("a lease request");
    assert_eq!(lease.window, ToplevelIdent::parse("w1").unwrap());
    assert_eq!(lease.mode, cua_action::RunMode::InPlace);
    assert_eq!(states(&d.effects), vec![cua_bus::RunStateSlug::Leasing]);

    for (name, input) in [
        (
            "refused",
            RunInput::Granted(GrantAnswer::Refused(ConfirmEnd::Dismissed)),
        ),
        (
            "the router is away",
            RunInput::GrantFailed(cua_run::LinkError::Unavailable),
        ),
    ] {
        let mut d = Drive::start(STOCK, task(model_says()));
        d.feed(input);
        assert_eq!(
            finished_with(&d),
            Some(CuaOutcome::Blocked(BlockedOn::ConsentDenied)),
            "{name}"
        );
    }
}

#[test]
fn the_lease_row_starts_the_first_look_and_the_capture_comes_last() {
    let mut d = Drive::start(STOCK, task(model_says()));
    d.feed(RunInput::Granted(GrantAnswer::Granted));
    d.feed(RunInput::Leased(grant(STOCK)));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(1)));
    assert_eq!(d.run.lease, Some(LeaseId(1)));
    assert_eq!(kinds(&d.effects), ["cua.run.started"]);
    assert!(d.has(|e| matches!(e, RunEffect::Glow(GlowState::Working))));
    assert!(d.has(|e| matches!(e, RunEffect::FetchContext)));
    assert!(d.has(|e| matches!(e, RunEffect::Snapshot(_))));
    assert!(d.has(|e| matches!(e, RunEffect::Emit(CuaSignal::StepStarted(StepIndex(1))))));
    assert_eq!(
        d.effects.last(),
        Some(&RunEffect::Capture {
            want: FrameWant::Now,
            hints: cua_run::HintSource::FromTree
        }),
        "the capture builds the observation, so it is last"
    );
    let mut refused = Drive::start(STOCK, task(model_says()));
    refused.feed(RunInput::Granted(GrantAnswer::Granted));
    refused.feed(RunInput::LeaseRefused(cua_desktop::DesktopError::Busy));
    assert_eq!(
        finished_with(&refused),
        Some(CuaOutcome::Blocked(BlockedOn::LeaseRefused))
    );
    assert!(
        !refused.has(|e| matches!(e, RunEffect::ReleaseLease(_))),
        "no lease was taken"
    );
}

#[test]
fn an_observation_goes_to_the_model_and_the_run_notes_what_it_read() {
    let mut d = Drive::leased(STOCK, task(model_says()));
    d.feed(RunInput::Observed(Box::new(observation(1, tree(vec![])))));
    assert_eq!(*d.phase(), RunPhase::Thinking(StepIndex(1)));
    let call = d.effects.iter().find_map(|e| match e {
        RunEffect::CallModel { request, .. } => Some(request.clone()),
        _ => None,
    });
    assert_eq!(call.expect("a model call").step, StepIndex(1));
    assert_eq!(d.run.used.steps, porter_core::Count(1));
    assert_eq!(d.run.frames.count(), porter_core::Count(1));
    assert_eq!(
        d.run.taint.integrity,
        prov::Integrity::Untrusted,
        "the goal was trusted; once the window is seen the run is not"
    );
}

#[test]
fn equal_frames_in_a_row_end_the_run_as_a_stall() {
    let mut d = Drive::leased(STOCK, task(model_says()));
    for _ in 0..2 {
        d.feed(RunInput::Observed(Box::new(observation(5, tree(vec![])))));
        d.feed(RunInput::ModelFailed(CuaModelError::Transient));
    }
    d.feed(RunInput::Observed(Box::new(observation(5, tree(vec![])))));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::BudgetOut(RunBudgetKind::Stall))
    );
}

#[test]
fn the_step_budget_ends_the_run_when_the_step_past_it_is_observed() {
    let mut t = task(model_says());
    t.budget.steps = porter_core::Count(1);
    let mut d = Drive::leased(STOCK, t);
    d.feed(RunInput::Observed(Box::new(observation(1, tree(vec![])))));
    assert_eq!(*d.phase(), RunPhase::Thinking(StepIndex(1)));
    d.feed(RunInput::ModelFailed(CuaModelError::Parse));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(1)));
    d.run.phase = RunPhase::Observing(StepIndex(2));
    d.feed(RunInput::Observed(Box::new(observation(2, tree(vec![])))));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::BudgetOut(RunBudgetKind::Steps))
    );
}

#[test]
fn a_failed_capture_is_tried_again_once_and_a_gone_window_ends_the_run() {
    let mut d = Drive::leased(STOCK, task(model_says()));
    d.feed(RunInput::CaptureFailed(CaptureError::Refused));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(1)));
    assert!(d.has(|e| matches!(e, RunEffect::Capture { .. })));
    d.feed(RunInput::CaptureFailed(CaptureError::Protocol("x".into())));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Failed(FailReason::Capture))
    );
    let mut gone = Drive::leased(STOCK, task(model_says()));
    gone.feed(RunInput::CaptureFailed(CaptureError::SourceGone));
    assert_eq!(
        finished_with(&gone),
        Some(CuaOutcome::Failed(FailReason::WindowGone))
    );
    assert!(gone.has(|e| matches!(e, RunEffect::ReleaseLease(LeaseId(1)))));
}

#[test]
fn what_the_model_says_while_thinking_is_announced_once() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelEvent(StepEvent::ActionProposed(click_at(
        1, 1,
    ))));
    assert!(d.has(|e| matches!(e, RunEffect::Emit(CuaSignal::ActionProposed(..)))));
    d.feed(RunInput::ModelReplied(reply(vec![
        click_at(1, 1),
        click_at(2, 2),
    ])));
    let announced = d
        .effects
        .iter()
        .filter(|e| matches!(e, RunEffect::Emit(CuaSignal::ActionProposed(..))))
        .count();
    assert_eq!(announced, 1, "the streamed one is not announced again");
}

#[test]
fn a_click_is_authorised_with_the_facts_the_router_needs() {
    let nodes = vec![
        node(0, a11y_tree::Role::Window, "Sign in", (0, 0, 100, 80)),
        node(1, a11y_tree::Role::Button, "Sign in", (10, 10, 40, 20)),
    ];
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(nodes));
    d.feed(RunInput::ModelReplied(reply(vec![click_at(20, 15)])));
    assert_eq!(*d.phase(), RunPhase::Authorising(StepIndex(1)));
    let ask = d.effects.iter().find_map(|e| match e {
        RunEffect::Check(ask) => Some(ask.clone()),
        _ => None,
    });
    let ask = ask.expect("a check");
    assert_eq!(ask.run, run_id());
    assert_eq!(ask.step, 1);
    assert_eq!(ask.app, app());
    assert_eq!(ask.trust, cua_action::WindowTrust::Native);
    assert_eq!(ask.effect, prov::Effect::UndoableWrite);
    assert_eq!(ask.basis, docket_core::EffectBasis::DefaultTable);
    assert_eq!(ask.screen.integrity, prov::Integrity::Untrusted);
    let node = ask.node.expect("the smallest node under the point");
    assert_eq!(node.role, "button");
    assert_eq!(node.name.value, "Sign in");
}

#[test]
fn a_target_outside_the_window_is_refused_locally_and_never_asked() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![
        click_at(500, 500),
        click_at(1, 1),
    ])));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert!(!d.has(|e| matches!(e, RunEffect::Check(_))));
    d.feed(RunInput::Observed(Box::new(observation(2, tree(vec![])))));
    let prev = &d.run.working.as_ref().unwrap().prev;
    assert_eq!(
        prev.iter().map(|o| o.result.clone()).collect::<Vec<_>>(),
        vec![
            ActionResult::Refused(Refusal::Local(LocalRefusal::OutsideLease)),
            ActionResult::NotExecuted
        ]
    );
}

#[test]
fn an_unknown_node_cannot_be_aimed() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![click_node(9)])));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    d.feed(RunInput::Observed(Box::new(observation(2, tree(vec![])))));
    let prev = &d.run.working.as_ref().unwrap().prev;
    assert_eq!(prev[0].result, ActionResult::Failed(SeatError::Refused));
}

#[test]
fn a_verdict_to_run_injects_and_the_input_settles_before_the_next_look() {
    let nodes = vec![
        node(0, a11y_tree::Role::Window, "Sign in", (0, 0, 100, 80)),
        node(1, a11y_tree::Role::Button, "Go", (10, 10, 40, 20)),
    ];
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(nodes));
    d.feed(RunInput::ModelReplied(reply(vec![
        click_node(1),
        click_at(2, 2),
    ])));
    d.feed(RunInput::Verdict(GateAnswer::Run));
    assert_eq!(*d.phase(), RunPhase::Acting(StepIndex(1)));
    let planned = d.effects.iter().find_map(|e| match e {
        RunEffect::Inject(p) => Some(p.clone()),
        _ => None,
    });
    let planned = planned.expect("an injection");
    assert_eq!(
        planned.at,
        seat_input::ResolvedPoints::One(cua_action::Point::new(Coord(30), Coord(20))),
        "a node is aimed at the centre of its bounds"
    );
    assert!(d.has(|e| matches!(e, RunEffect::Glow(GlowState::Acting { .. }))));
    d.feed_after(100, RunInput::Injected(receipt(110)));
    assert!(matches!(
        d.phase(),
        RunPhase::Settling(StepIndex(1), Settle::Waiting { .. })
    ));
    assert_eq!(kinds(&d.effects), ["cua.step"]);
    assert!(d.has(|e| matches!(e, RunEffect::WatchSettle { .. })));
    assert!(d.has(|e| matches!(e, RunEffect::Emit(CuaSignal::ActionDone(..)))));
    assert!(
        d.run.pending.actions.is_empty(),
        "the rest of the batch is dropped"
    );
    assert_eq!(
        d.run.track.outcomes.last().map(|o| &o.result),
        Some(&ActionResult::NotExecuted),
        "the model is told the second click did not run"
    );
}

#[test]
fn settling_waits_while_the_screen_changes_then_looks_again() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![
        click_at(1, 1),
        click_at(2, 2),
    ])));
    d.feed(RunInput::Verdict(GateAnswer::Run));
    d.feed_after(0, RunInput::Injected(receipt(0)));
    let acted = d.at;
    d.feed_after(100, RunInput::Damage(damage(acted.0 + 100)));
    let RunPhase::Settling(_, Settle::Waiting { wake }) = *d.phase() else {
        panic!("{:?}", d.phase())
    };
    assert_eq!(
        wake,
        Stamp(acted.0 + 100 + 250),
        "the quiet period restarts"
    );
    assert_eq!(d.run.wake(), Some(wake));
    d.feed_after(250, RunInput::Tick);
    assert_eq!(
        *d.phase(),
        RunPhase::Observing(StepIndex(2)),
        "the rest of the batch was dropped, so the run looks before it acts again"
    );
    assert!(d.has(|e| matches!(e, RunEffect::CancelSettle)));
    assert!(!d.has(|e| matches!(e, RunEffect::Check(_))));
}

#[test]
fn an_empty_queue_after_settling_looks_again_after_the_damage() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![click_at(1, 1)])));
    d.feed(RunInput::Verdict(GateAnswer::Run));
    d.feed_after(0, RunInput::Injected(receipt(0)));
    d.feed_after(300, RunInput::Tick);
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(
        d.effects.last(),
        Some(&RunEffect::Capture {
            want: FrameWant::AfterDamage,
            hints: cua_run::HintSource::FromTree
        })
    );
    assert!(d.has(|e| matches!(e, RunEffect::Emit(CuaSignal::StepStarted(StepIndex(2))))));
    assert_eq!(d.run.track.settle, SettleWatch::Idle);
}

#[test]
fn a_screen_that_never_stops_changing_times_out() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![click_at(1, 1)])));
    d.feed(RunInput::Verdict(GateAnswer::Run));
    d.feed_after(0, RunInput::Injected(receipt(0)));
    let since = d.at.0;
    for i in 1..40 {
        d.feed_after(100, RunInput::Damage(damage(since + i * 100)));
        if matches!(d.phase(), RunPhase::Observing(_)) {
            break;
        }
    }
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert!(
        d.at.0 - since >= 3000,
        "only after the timeout: {}",
        d.at.0 - since
    );
}

#[test]
fn a_wait_lasts_the_time_asked_but_never_longer_than_the_setting() {
    for (asked, slept) in [(500, 500_u64), (50_000, 5000)] {
        let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
        d.run.env.wait_max = cua_desktop::Ms(5000);
        d.feed(RunInput::ModelReplied(reply(vec![wait(asked)])));
        let at = d.at.0;
        assert_eq!(
            *d.phase(),
            RunPhase::Settling(
                StepIndex(1),
                Settle::Waiting {
                    wake: Stamp(at + slept)
                }
            )
        );
        d.feed_after(slept - 1, RunInput::Tick);
        assert!(matches!(d.phase(), RunPhase::Settling(..)), "not yet");
        d.feed_after(1, RunInput::Tick);
        assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
        assert!(d.has(|e| matches!(e, RunEffect::CancelSettle)));
        assert_eq!(d.run.track.settle, SettleWatch::Idle);
    }
}

#[test]
fn a_wait_ends_early_once_the_window_has_changed_and_gone_quiet() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![wait(30_000)])));
    assert!(d.has(|e| matches!(e, RunEffect::WatchSettle { .. })));
    let began = d.at.0;
    d.feed_after(1000, RunInput::Damage(damage(began + 1000)));
    assert_eq!(
        *d.phase(),
        RunPhase::Settling(
            StepIndex(1),
            Settle::Waiting {
                wake: Stamp(began + 1000 + 250)
            }
        ),
        "the quiet period after the change, far short of the deadline"
    );
    d.feed_after(100, RunInput::Damage(damage(began + 1100)));
    d.feed_after(250, RunInput::Tick);
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(
        d.effects.last(),
        Some(&RunEffect::Capture {
            want: FrameWant::AfterDamage,
            hints: cua_run::HintSource::FromTree
        })
    );
}

#[test]
fn a_long_wait_is_not_a_stall_until_the_total_is_spent() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    let total = d.run.env.wait_total.0;
    let mut waited = 0;
    let mut waits = 0;
    while finished_with(&d).is_none() && waits < 30 {
        d.feed(RunInput::ModelReplied(reply(vec![wait(60_000)])));
        d.feed_after(60_000, RunInput::Tick);
        d.feed(RunInput::Observed(Box::new(observation(1, tree(vec![])))));
        waits += 1;
        waited += 60_000;
        if waited <= total {
            assert_eq!(finished_with(&d), None, "wait {waits} is within the total");
        }
    }
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::BudgetOut(RunBudgetKind::Stall))
    );
    assert!(waited > total, "only after the total: {waited} ms");
}

#[test]
fn the_person_can_stop_a_run_in_the_middle_of_a_long_wait() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![wait(60_000)])));
    d.feed_after(5000, RunInput::UserCancel);
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Cancelled(cua_bus::CancelledBy::User))
    );
    assert!(d.has(|e| matches!(e, RunEffect::CancelSettle)));
    assert_eq!(d.run.track.settle, SettleWatch::Idle);
}

#[test]
fn zoom_and_observe_just_look_again_and_drop_the_rest() {
    for look in [
        CuaAction::Observe,
        CuaAction::Zoom {
            region: Rect {
                origin: cua_action::Point::new(Coord(0), Coord(0)),
                size: Size::new(Coord(10), Coord(10)),
            },
        },
    ] {
        let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
        d.feed(RunInput::ModelReplied(reply(vec![look, click_at(1, 1)])));
        assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
        assert!(d.run.pending.actions.is_empty());
        assert!(!d.has(|e| matches!(e, RunEffect::Check(_))));
    }
}

#[test]
fn finish_done_on_the_models_word_ends_the_run_and_reports_done_with_the_values() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    let finish = CuaAction::Finish {
        outcome: FinishOutcome::Done,
        summary: Summary::new("Downloaded it.").unwrap(),
        extracted: vec![Extracted {
            name: FieldName::new("invoice_no").unwrap(),
            value: ExtractedText::new("2291").unwrap(),
        }],
    };
    d.feed(RunInput::ModelReplied(reply(vec![finish])));
    assert_eq!(finished_with(&d), Some(CuaOutcome::Done));
    let (status, text) = report_of(&d.effects).expect("a report");
    assert_eq!(status, ReportStatus::Done);
    assert_eq!(text, ["Downloaded it.", "invoice_no: 2291"]);
    assert_eq!(kinds(&d.effects), ["cua.run.finished"]);
    let order: Vec<&str> = d
        .effects
        .iter()
        .map(|e| match e {
            RunEffect::Glow(_) => "glow",
            RunEffect::ReleaseLease(_) => "release",
            RunEffect::Report(_) => "report",
            RunEffect::Record(_) => "record",
            RunEffect::Emit(CuaSignal::Finished(_)) => "finished",
            _ => "",
        })
        .filter(|s| !s.is_empty())
        .collect();
    assert_eq!(order, ["glow", "release", "report", "record", "finished"]);
    assert_eq!(d.run.lease, None);
}

#[test]
fn finish_failed_or_infeasible_ends_failed() {
    for outcome in [FinishOutcome::Failed, FinishOutcome::Infeasible] {
        let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
        d.feed(RunInput::ModelReplied(reply(vec![finish(
            outcome,
            "No such page.",
        )])));
        assert_eq!(
            finished_with(&d),
            Some(CuaOutcome::Failed(FailReason::Infeasible))
        );
        assert_eq!(report_of(&d.effects).unwrap().0, ReportStatus::Failed);
        assert_eq!(report_of(&d.effects).unwrap().1, ["No such page."]);
    }
}

#[test]
fn a_success_check_on_the_tree_must_pass_before_done() {
    let selector = a11y_tree::NodeSelector { path: vec![] };
    for check in [
        SuccessCheck::TreeHas(selector.clone()),
        SuccessCheck::Both(selector.clone()),
    ] {
        let mut d = Drive::thinking(STOCK, task(check), tree(vec![]));
        d.feed(RunInput::ModelReplied(reply(vec![finish(
            FinishOutcome::Done,
            "ok",
        )])));
        assert_eq!(*d.phase(), RunPhase::Verifying(StepIndex(1)));
        assert!(d.has(|e| matches!(e, RunEffect::VerifyTree(_))));
        let mut failing = d.run.clone();
        d.feed(RunInput::Checked(CheckResult::Pass));
        assert_eq!(finished_with(&d), Some(CuaOutcome::Done));
        assert_eq!(report_of(&d.effects).unwrap().1, ["ok"]);

        let (run, effects) = {
            failing.last_at = Stamp(0);
            failing.step(RunInput::Checked(CheckResult::Fail), Stamp(5))
        };
        assert_eq!(run.phase, RunPhase::Observing(StepIndex(2)));
        assert!(
            effects
                .iter()
                .any(|e| matches!(e, RunEffect::Capture { .. }))
        );
        assert_eq!(
            run.track.outcomes.last().map(|o| o.result.clone()),
            Some(ActionResult::CheckFailed)
        );
    }
}

#[test]
fn a_question_waits_for_the_person_and_the_answer_goes_back_as_the_outcome() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    let ask = CuaAction::Ask {
        question: Summary::new("Which account?").unwrap(),
        choices: vec![Choice::new("Work").unwrap(), Choice::new("Home").unwrap()],
    };
    d.feed(RunInput::ModelReplied(reply(vec![ask, click_at(1, 1)])));
    let q = QuestionId(1);
    assert_eq!(*d.phase(), RunPhase::WaitingForUser(StepIndex(1), q));
    let body = d.effects.iter().find_map(|e| match e {
        RunEffect::Emit(CuaSignal::AskUser(id, body)) => Some((*id, body.clone())),
        _ => None,
    });
    let (id, body) = body.expect("an AskUser signal");
    assert_eq!(id, q);
    assert_eq!(body.question.value, "Which account?");
    assert_eq!(body.question.label.integrity, prov::Integrity::Untrusted);
    assert_eq!(body.choices.len(), 2);
    assert_eq!(kinds(&d.effects), ["cua.asked"]);
    assert!(d.has(|e| matches!(e, RunEffect::Glow(GlowState::WaitingForYou))));
    assert_eq!(report_of(&d.effects).unwrap().0, ReportStatus::Progress);

    d.feed(RunInput::UserAnswer(QuestionId(99), user_goal()));
    assert!(
        matches!(d.phase(), RunPhase::WaitingForUser(..)),
        "another question's answer is not this one's"
    );
    d.feed(RunInput::UserAnswer(
        q,
        Labelled::new("Work".into(), user_goal().label),
    ));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(kinds(&d.effects), ["cua.step"]);
    assert!(
        matches!(&d.run.track.outcomes[..], [ActionOutcome { result: ActionResult::Answered(a), .. }] if a.value == "Work"),
        "the rest of the batch is dropped, the answer is the outcome"
    );
}

#[test]
fn model_failures_are_counted_and_the_limit_ends_the_run() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelFailed(CuaModelError::Transient));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(1)));
    d.feed(RunInput::Observed(Box::new(observation(2, tree(vec![])))));
    d.feed(RunInput::ModelFailed(CuaModelError::Parse));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(1)));
    d.feed(RunInput::Observed(Box::new(observation(3, tree(vec![])))));
    d.feed(RunInput::ModelFailed(CuaModelError::Transient));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Failed(FailReason::Model)),
        "the third failure is over the limit of two"
    );
    let mut fatal = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    fatal.feed(RunInput::ModelFailed(CuaModelError::Fatal));
    assert_eq!(
        finished_with(&fatal),
        Some(CuaOutcome::Failed(FailReason::Model))
    );
    assert!(fatal.has(|e| matches!(e, RunEffect::CancelModel)));
}

#[test]
fn an_empty_reply_and_a_vendor_hint_count_as_failures_and_look_again_at_the_next_step() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![])));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(d.run.used.model_failures, porter_core::Count(1));
    let mut hinted = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    let mut r = reply(vec![click_at(1, 1)]);
    r.reply.safety = vec![porter_infer::SafetyHint::Blocked("no".into())];
    hinted.feed(RunInput::ModelReplied(r));
    assert_eq!(*hinted.phase(), RunPhase::Observing(StepIndex(2)));
    assert!(
        !hinted.has(|e| matches!(e, RunEffect::Check(_))),
        "nothing runs under a hint"
    );
}

#[test]
fn spend_over_the_budget_ends_the_run_after_the_reply() {
    let mut t = task(model_says());
    t.budget.spend = porter_core::MicroUsd(100);
    let mut d = Drive::thinking(STOCK, t, tree(vec![]));
    let mut r = reply(vec![click_at(1, 1)]);
    r.cost = porter_core::MicroUsd(101);
    d.feed(RunInput::ModelReplied(r));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::BudgetOut(RunBudgetKind::Spend))
    );
    let mut ok = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    ok.feed(RunInput::ModelReplied(reply(vec![click_at(1, 1)])));
    assert!(
        matches!(ok.phase(), RunPhase::Authorising(_)),
        "a local model costs nothing"
    );
}

#[test]
fn a_refusal_marks_the_rest_not_executed_and_tells_the_model() {
    let mut d = authorising();
    d.feed(RunInput::Verdict(GateAnswer::Refused(CallRefusal::Denied(
        DenyCode::NeedsUser,
    ))));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(kinds(&d.effects), ["cua.step"]);
    d.feed(RunInput::Observed(Box::new(observation(2, tree(vec![])))));
    let prev: Vec<ActionResult> = d
        .run
        .working
        .as_ref()
        .unwrap()
        .prev
        .iter()
        .map(|o| o.result.clone())
        .collect();
    assert_eq!(
        prev,
        vec![
            ActionResult::Refused(Refusal::Gate(CallRefusal::Denied(DenyCode::NeedsUser))),
            ActionResult::NotExecuted,
            ActionResult::NotExecuted
        ]
    );
}

#[test]
fn a_paused_session_blocks_the_run_and_a_halted_space_pauses_it() {
    let mut d = authorising();
    d.feed(RunInput::Verdict(GateAnswer::Refused(CallRefusal::Paused(
        BreakerTrip::Consecutive,
    ))));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Blocked(BlockedOn::PolicyDenials))
    );
    let mut halted = authorising();
    halted.feed(RunInput::Verdict(GateAnswer::Refused(CallRefusal::Halted(
        SpaceScope::Any,
    ))));
    assert_eq!(*halted.phase(), RunPhase::Paused(StepIndex(1)));
    assert!(halted.run.pending.actions.is_empty());
}

#[test]
fn a_router_that_cannot_be_asked_blocks_the_run() {
    let mut d = authorising();
    d.feed(RunInput::GateFailed(cua_run::LinkError::Unavailable));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Blocked(BlockedOn::PolicyDenials))
    );
}

#[test]
fn a_confirmation_that_is_granted_runs_the_action_and_resumes_input_on_stock() {
    let mut d = authorising();
    d.feed(RunInput::Confirming(ConfirmId::parse("c-1").unwrap()));
    assert!(
        d.has(|e| matches!(e, RunEffect::SuspendLease(_, Suspend::Consent))),
        "stock suspends input while the person is asked"
    );
    assert!(d.has(|e| matches!(e, RunEffect::Proceed)));
    d.feed(RunInput::Verdict(GateAnswer::Run));
    assert_eq!(*d.phase(), RunPhase::Acting(StepIndex(1)));
    let resume = d
        .effects
        .iter()
        .position(|e| matches!(e, RunEffect::ResumeLease(LeaseId(1))));
    let inject = d
        .effects
        .iter()
        .position(|e| matches!(e, RunEffect::Inject(_)));
    assert!(
        resume.unwrap() < inject.unwrap(),
        "input flows before the click is made"
    );
    assert_eq!(kinds(&d.effects), ["cua.confirmed"]);
    assert_eq!(d.run.track.authorised, cua_bus::AuthorisedBy::User);
    d.feed(RunInput::Injected(receipt(50)));
    let step = d.effects.iter().find_map(|e| match e {
        RunEffect::Record(r) => match &**r {
            CuaRecord::Step { authorised_by, .. } => Some(*authorised_by),
            _ => None,
        },
        _ => None,
    });
    assert_eq!(step, Some(cua_bus::AuthorisedBy::User));
}

#[test]
fn a_confirmation_that_is_declined_is_told_to_the_model_as_declined() {
    let mut d = authorising();
    d.feed(RunInput::Confirming(ConfirmId::parse("c-1").unwrap()));
    d.feed(RunInput::Verdict(GateAnswer::Refused(
        CallRefusal::Unconfirmed(ConfirmEnd::Refused),
    )));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    d.feed(RunInput::Observed(Box::new(observation(2, tree(vec![])))));
    assert_eq!(
        d.run.working.as_ref().unwrap().prev[0].result,
        ActionResult::UserDeclined
    );
}

#[test]
fn the_rate_limit_ends_the_run_before_the_action_that_would_pass_it() {
    let mut t = task(model_says());
    t.budget.actions_per_min = porter_core::Count(2);
    let mut d = Drive::thinking(STOCK, t, tree(vec![]));
    d.feed(RunInput::ModelReplied(reply(vec![click_at(1, 1)])));
    for i in 0..2_u8 {
        d.feed(RunInput::Verdict(GateAnswer::Run));
        d.feed(RunInput::Injected(receipt(0)));
        d.feed_after(300, RunInput::Tick);
        d.feed(RunInput::Observed(Box::new(observation(
            10 + i,
            tree(vec![]),
        ))));
        d.feed(RunInput::ModelReplied(reply(vec![click_at(3, 3)])));
    }
    d.feed(RunInput::Verdict(GateAnswer::Run));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::BudgetOut(RunBudgetKind::Rate))
    );
}

#[test]
fn input_that_cannot_be_delivered_looks_again_unless_the_window_is_gone() {
    let mut d = authorising();
    d.feed(RunInput::Verdict(GateAnswer::Run));
    d.feed(RunInput::InjectFailed(SeatError::Refused));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(kinds(&d.effects), ["cua.step"]);
    let mut gone = authorising();
    gone.feed(RunInput::Verdict(GateAnswer::Run));
    gone.feed(RunInput::InjectFailed(SeatError::TargetGone));
    assert_eq!(
        finished_with(&gone),
        Some(CuaOutcome::Failed(FailReason::WindowGone))
    );
}

#[test]
fn a_window_that_moved_under_the_aim_is_not_injected_into_and_the_run_looks_again() {
    let mut d = authorising();
    d.feed(RunInput::Verdict(GateAnswer::Run));
    d.feed(RunInput::StateMoved(cua_run::Moved::Focus));
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(kinds(&d.effects), ["cua.step"]);
}

#[test]
fn the_bus_is_told_how_many_equal_frames_came_in_a_row() {
    let mut d = Drive::leased(STOCK, task(model_says()));
    d.feed(RunInput::Observed(Box::new(observation(5, tree(vec![])))));
    assert_eq!(d.run.used.stalled, porter_core::Count(1));
    d.feed(RunInput::ModelFailed(CuaModelError::Transient));
    d.feed(RunInput::Observed(Box::new(observation(5, tree(vec![])))));
    assert_eq!(d.run.used.stalled, porter_core::Count(2));
    d.feed(RunInput::ModelFailed(CuaModelError::Transient));
    d.feed(RunInput::Observed(Box::new(observation(6, tree(vec![])))));
    assert_eq!(
        d.run.used.stalled,
        porter_core::Count(1),
        "a new frame starts again"
    );
}

#[test]
fn an_injection_receipt_with_no_action_waiting_fails_the_run_instead_of_panicking() {
    let mut d = Drive::leased(STOCK, task(model_says()));
    d.feed(RunInput::Observed(Box::new(observation(1, tree(vec![])))));
    d.run.phase = RunPhase::Acting(StepIndex(1));
    d.run.pending.actions.clear();
    d.feed(RunInput::Injected(receipt(0)));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Failed(cua_bus::FailReason::Input))
    );
    let mut d = Drive::leased(STOCK, task(model_says()));
    d.feed(RunInput::Observed(Box::new(observation(1, tree(vec![])))));
    d.run.phase = RunPhase::Acting(StepIndex(1));
    d.run.pending.actions.clear();
    d.feed(RunInput::InjectFailed(seat_input::SeatError::Refused));
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Failed(cua_bus::FailReason::Input))
    );
}

#[test]
fn a_sensitive_window_asks_the_person_even_when_the_router_says_run() {
    for (answer, goes_on) in [(cua_run::FORCED_YES, true), (cua_run::FORCED_NO, false)] {
        let password = node(1, a11y_tree::Role::PasswordText, "Password", (0, 0, 50, 20));
        let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![password]));
        d.feed(RunInput::ModelReplied(reply(vec![
            click_at(1, 1),
            click_at(2, 2),
        ])));
        d.feed(RunInput::Verdict(GateAnswer::Run));
        let q = QuestionId(1);
        assert_eq!(*d.phase(), RunPhase::WaitingForUser(StepIndex(1), q));
        assert!(d.has(|e| matches!(e, RunEffect::Emit(CuaSignal::AskUser(..)))));
        assert!(!d.has(|e| matches!(e, RunEffect::Inject(_))), "nothing ran");
        assert_eq!(
            d.run.pending.actions.len(),
            2,
            "the step waits in the queue"
        );

        d.feed(RunInput::UserAnswer(
            q,
            Labelled::new(answer.to_owned(), user_goal().label),
        ));
        if goes_on {
            assert_eq!(*d.phase(), RunPhase::Acting(StepIndex(1)));
            assert!(d.has(|e| matches!(e, RunEffect::Inject(_))));
            assert!(d.has(|e| matches!(
                e,
                RunEffect::Record(r) if matches!(**r, CuaRecord::Confirmed { .. })
            )));
        } else {
            assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
            assert!(!d.has(|e| matches!(e, RunEffect::Inject(_))));
            assert!(d.run.pending.actions.is_empty());
            assert!(matches!(
                d.run.track.outcomes.first().map(|o| &o.result),
                Some(ActionResult::UserDeclined)
            ));
        }
    }
}
