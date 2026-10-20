//! The rows of the phase table that the person, the shell and the desktop drive: pause, resume,
//! take over, hand back, cancel, the kill chord, the active budget; and the table's invariants.

use crate::support::*;
use cua_action::{Coord, Rect, Size};
use cua_bus::{
    ActionResult, BlockedOn, CancelledBy, CuaOutcome, FailReason, QuestionId, RunBudgetKind,
    SuccessCheck,
};
use cua_desktop::{
    BackendKind, DesktopEvent, GlowState, LeaseId, RevokeReason, Stamp, Suspend, ToplevelIdent,
};
use cua_run::{CheckResult, CuaModelError, RunEffect, RunInput, RunPhase, StepEvent};
use docket_core::{GateAnswer, GrantAnswer};
use maskcap::{CaptureError, DamageAt, DamageSource, Settle};
use porter_infer::StepIndex;
use prov::{ConfirmId, MessageKind, ReportStatus};
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

fn finished_with(d: &Drive) -> Option<CuaOutcome> {
    match d.phase() {
        RunPhase::Finished(o) => Some(*o),
        _ => None,
    }
}

#[test]
fn pausing_stops_what_the_phase_waited_on_and_the_requester_hears_it() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::UserPause);
    assert_eq!(*d.phase(), RunPhase::Paused(StepIndex(1)));
    assert!(d.has(|e| matches!(e, RunEffect::CancelModel)));
    assert!(d.has(|e| matches!(e, RunEffect::SuspendLease(_, Suspend::Paused))));
    assert_eq!(report_of(&d.effects).unwrap().0, ReportStatus::Progress);
    let again = d.effects.clone();
    d.feed(RunInput::UserPause);
    assert!(d.effects.is_empty(), "already paused: {again:?}");
    d.feed(RunInput::UserResume);
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert!(d.has(|e| matches!(e, RunEffect::Glow(GlowState::Working))));
    d.feed(RunInput::UserResume);
    assert!(d.effects.is_empty(), "resume of a run that is not paused");
}

#[test]
fn time_spent_paused_or_in_a_confirmation_is_not_the_runs() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    let base = d.run.used.active.0;
    d.feed_after(1000, RunInput::UserPause);
    let before = d.run.used.active;
    assert_eq!(before, cua_desktop::Ms(base + 1000));
    d.feed_after(3_600_000, RunInput::UserResume);
    assert_eq!(d.run.used.active, before, "an hour paused costs nothing");
    d.feed_after(
        500,
        RunInput::Observed(Box::new(observation(9, tree(vec![])))),
    );
    assert_eq!(d.run.used.active, cua_desktop::Ms(base + 1500));

    let mut c = authorising();
    let spent = c.run.used.active;
    c.feed(RunInput::Confirming(ConfirmId::parse("c-1").unwrap()));
    c.feed_after(600_000, RunInput::Tick);
    assert_eq!(
        c.run.used.active.0,
        spent.0 + 10,
        "only the ten ms before the sheet"
    );
    assert!(!c.phase().is_finished());
}

#[test]
fn a_tick_past_the_active_budget_ends_the_run_and_wake_names_the_deadline() {
    let mut t = task(model_says());
    t.budget.active = cua_desktop::Ms(1000);
    let mut d = Drive::thinking(STOCK, t, tree(vec![]));
    let used = d.run.used.active.0;
    assert_eq!(d.run.wake(), Some(Stamp(d.at.0 + 1000 - u64::from(used))));
    let deadline = d.run.wake().unwrap();
    d.feed_after(deadline.0 - d.at.0, RunInput::Tick);
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::BudgetOut(RunBudgetKind::Active))
    );
    assert!(d.has(|e| matches!(e, RunEffect::CancelModel)));
    assert_eq!(d.run.wake(), None);
}

#[test]
fn taking_over_stops_the_input_and_handing_back_tells_the_model_the_person_acted() {
    let mut d = authorising();
    d.feed(RunInput::UserTakeOver);
    let RunPhase::TakenOver(n, since) = *d.phase() else {
        panic!("{:?}", d.phase())
    };
    assert_eq!(n, StepIndex(1));
    assert!(d.has(|e| matches!(e, RunEffect::SuspendLease(_, Suspend::TakeOver))));
    assert!(d.has(|e| matches!(e, RunEffect::Glow(GlowState::Off))));
    assert_eq!(kinds(&d.effects), ["cua.taken_over"]);
    d.feed(RunInput::UserTakeOver);
    assert!(d.effects.is_empty());
    d.feed(RunInput::UserHandBack);
    assert_eq!(*d.phase(), RunPhase::Observing(StepIndex(2)));
    assert_eq!(kinds(&d.effects), ["cua.handed_back"]);
    assert!(
        d.has(|e| matches!(e, RunEffect::FetchContext)),
        "a fresh look at what they did"
    );
    assert!(
        d.run
            .track
            .outcomes
            .iter()
            .any(|o| o.result == ActionResult::UserActed { since }),
    );
}

#[test]
fn cancel_the_kill_chord_and_a_revoked_lease_each_end_the_run_their_own_way() {
    let cases = [
        (
            RunInput::UserCancel,
            CuaOutcome::Cancelled(CancelledBy::User),
            true,
        ),
        (
            RunInput::Desktop(DesktopEvent::KillChord),
            CuaOutcome::Cancelled(CancelledBy::KillSwitch),
            true,
        ),
        (
            RunInput::Desktop(DesktopEvent::LeaseRevoked {
                lease: LeaseId(1),
                why: RevokeReason::KillChord,
            }),
            CuaOutcome::Cancelled(CancelledBy::KillSwitch),
            false,
        ),
        (
            RunInput::Desktop(DesktopEvent::LeaseRevoked {
                lease: LeaseId(1),
                why: RevokeReason::WindowClosed,
            }),
            CuaOutcome::Failed(FailReason::WindowGone),
            false,
        ),
        (
            RunInput::Desktop(DesktopEvent::WindowClosed(
                ToplevelIdent::parse("w1").unwrap(),
            )),
            CuaOutcome::Failed(FailReason::WindowGone),
            true,
        ),
    ];
    for (input, want, releases) in cases {
        let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
        d.feed(input.clone());
        assert_eq!(finished_with(&d), Some(want), "{input:?}");
        assert_eq!(
            d.has(|e| matches!(e, RunEffect::ReleaseLease(_))),
            releases,
            "{input:?}: a lease the desktop already took back is not released again"
        );
    }
    for ignored in [
        DesktopEvent::WindowClosed(ToplevelIdent::parse("other").unwrap()),
        DesktopEvent::LeaseRevoked {
            lease: LeaseId(7),
            why: RevokeReason::Compositor,
        },
        DesktopEvent::ConsentShown,
    ] {
        let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
        d.feed(RunInput::Desktop(ignored.clone()));
        assert!(!d.phase().is_finished(), "{ignored:?}");
    }
}

#[test]
fn physical_input_pauses_a_run_on_a_shared_seat_only_while_autopause_is_on() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    d.feed(RunInput::Desktop(DesktopEvent::PhysicalInput {
        lease: LeaseId(1),
    }));
    assert!(matches!(d.phase(), RunPhase::Paused(_)));

    let mut off = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    off.run.env.autopause = cua_run::Autopause::Off;
    off.feed(RunInput::Desktop(DesktopEvent::PhysicalInput {
        lease: LeaseId(1),
    }));
    assert!(matches!(off.phase(), RunPhase::Thinking(_)));
}

#[test]
fn inferd_saying_who_answers_is_recorded_once() {
    let mut d = Drive::thinking(STOCK, task(model_says()), tree(vec![]));
    let served = porter_infer::ServedBy {
        account: porter_core::AccountId::parse("local").unwrap(),
        model: porter_core::ModelId::parse("qwen3-vl").unwrap(),
        locality: porter_core::Locality::OnDevice,
    };
    d.feed(RunInput::Routed(served.clone()));
    assert_eq!(kinds(&d.effects), ["cua.run.routed"]);
    assert_eq!(d.run.served, Some(served));
    assert!(matches!(d.phase(), RunPhase::Thinking(_)));
}

#[test]
fn every_input_in_every_phase_has_a_row_and_leaves_a_valid_run() {
    // Not a table of outcomes: every input fed to every phase must not panic, and a finished
    // run stays finished.
    let inputs = |_: ()| -> Vec<RunInput> {
        vec![
            RunInput::Granted(GrantAnswer::Granted),
            RunInput::GrantFailed(cua_run::LinkError::Unavailable),
            RunInput::GrantUnanswered,
            RunInput::Leased(grant(STOCK)),
            RunInput::LeaseRefused(cua_desktop::DesktopError::Busy),
            RunInput::Observed(Box::new(observation(4, tree(vec![])))),
            RunInput::CaptureFailed(CaptureError::Refused),
            RunInput::ModelEvent(StepEvent::ThoughtDelta(cua_run::ThoughtText("x".into()))),
            RunInput::ModelReplied(reply(vec![click_at(1, 1)])),
            RunInput::ModelFailed(CuaModelError::Transient),
            RunInput::ScreenRead(cua_run::ScreenRead::Instructions),
            RunInput::Confirming(ConfirmId::parse("c-9").unwrap()),
            RunInput::Verdict(GateAnswer::Run),
            RunInput::GateFailed(cua_run::LinkError::Unavailable),
            RunInput::Injected(receipt(1)),
            RunInput::InjectFailed(SeatError::Refused),
            RunInput::StateMoved(cua_run::Moved::Geometry),
            RunInput::RecordFailed,
            RunInput::Damage(damage(1)),
            RunInput::Checked(CheckResult::Pass),
            RunInput::UserPause,
            RunInput::UserResume,
            RunInput::UserCancel,
            RunInput::UserAnswer(QuestionId(1), user_goal()),
            RunInput::UserTakeOver,
            RunInput::UserHandBack,
            RunInput::Desktop(DesktopEvent::KillChord),
            RunInput::Tick,
        ]
    };
    let phases = [
        RunPhase::Granting,
        RunPhase::Leasing,
        RunPhase::Observing(StepIndex(2)),
        RunPhase::Thinking(StepIndex(2)),
        RunPhase::Authorising(StepIndex(2)),
        RunPhase::Confirming(StepIndex(2), ConfirmId::parse("c-1").unwrap()),
        RunPhase::Acting(StepIndex(2)),
        RunPhase::Settling(StepIndex(2), Settle::Quiet),
        RunPhase::Verifying(StepIndex(2)),
        RunPhase::WaitingForUser(StepIndex(2), QuestionId(2)),
        RunPhase::Paused(StepIndex(2)),
        RunPhase::TakenOver(StepIndex(2), Stamp(1)),
    ];
    for phase in phases {
        for input in inputs(()) {
            let mut d = Drive::leased(STOCK, task(model_says()));
            d.feed(RunInput::Observed(Box::new(observation(1, tree(vec![])))));
            d.run.phase = phase.clone();
            d.run.pending.actions = vec![targeted_click()].into();
            d.feed(input.clone());
            if let RunPhase::Finished(_) = d.phase() {
                d.feed(input.clone());
                assert!(d.effects.is_empty(), "{phase:?} {input:?}");
            }
        }
    }
}

fn targeted_click() -> cua_bus::TargetedAction {
    cua_bus::TargetedAction {
        window: cua_bus::WindowRef(1),
        action: click_at(1, 1),
    }
}

#[test]
fn the_mode_and_the_target_decide_at_the_start_what_a_run_may_do() {
    let mut app_task = task(model_says());
    app_task.target = cua_bus::TaskTarget::App(app());
    let d = Drive::start(STOCK, app_task);
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Blocked(BlockedOn::NeedsCapability))
    );
    let mut hidden = task(model_says());
    hidden.mode = cua_action::RunMode::AgentWorkspace;
    let d = Drive::start(STOCK, hidden);
    assert_eq!(
        finished_with(&d),
        Some(CuaOutcome::Blocked(BlockedOn::ModeUnsupported(
            cua_action::RunMode::AgentWorkspace
        ))),
        "stock cannot run the agent workspace"
    );
}
