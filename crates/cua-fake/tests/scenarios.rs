//! Runs over the fakes beyond the happy path: the consent guard, a denial, a refused consent,
//! a message that reaches the model, a masked secret, a run that ends the way a fake says.

mod support;

use cua_action::CuaAction;
use cua_bus::{BlockedOn, CuaOutcome, CuaRecord, FailReason};
use cua_desktop::{BackendKind, Suspend};
use cua_fake::{DesktopCall, FakePdp, SeatEvent, Verdict, run_harness};
use cua_run::{CuaModelError, CuaSignal, RunPhase};
use docket_core::{CallRefusal, ConfirmEnd, DenyCode, GateAnswer, GrantAnswer, InboundLine};
use porter_infer::{NoteFrom, PrevResult, StepNote};
use prov::{Address, AgentRef, Crossing, MessageId, MessageKind, ReportStatus, ThreadId};
use seat_input::Emulation;
use support::*;

fn outcome_of(report: &cua_fake::HarnessReport) -> CuaOutcome {
    match report.run.phase {
        RunPhase::Finished(outcome) => outcome,
        ref other => panic!("not finished: {other:?}"),
    }
}

#[test]
fn on_stock_input_stops_before_the_prompt_shows_and_starts_after_the_answer() {
    let pdp = FakePdp::scripted(vec![], vec![Verdict::AskThen(GateAnswer::Run)]);
    let mut w = world(
        BackendKind::CosmicStock,
        vec![reply(click()), reply(finish())],
        pdp,
    );
    let report = block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).expect("ends");
    assert_eq!(outcome_of(&report), CuaOutcome::Done);
    let seat: Vec<&'static str> = w
        .seat
        .events()
        .iter()
        .map(|e| match e {
            SeatEvent::Attached(_) => "attach",
            SeatEvent::Emulation(_, Emulation::Stopped) => "stopped",
            SeatEvent::Emulation(_, Emulation::Running) => "running",
            SeatEvent::Performed(..) => "performed",
            SeatEvent::Detached(_) => "detach",
        })
        .collect();
    assert_eq!(
        seat,
        ["attach", "stopped", "running", "performed", "detach"],
        "nothing is injected while the prompt is up"
    );
    assert!(
        w.desktop
            .calls()
            .iter()
            .any(|c| matches!(c, DesktopCall::Suspended(_, Suspend::Consent)))
    );
    let kinds: Vec<String> = w.events.records().iter().map(CuaRecord::kind).collect();
    assert_eq!(
        kinds,
        [
            "cua.run.started",
            "cua.confirmed",
            "cua.step",
            "cua.run.finished"
        ]
    );
}

#[test]
fn a_refused_action_is_told_to_the_model_and_the_run_goes_on() {
    let pdp = FakePdp::scripted(
        vec![],
        vec![Verdict::Refuse(CallRefusal::Denied(DenyCode::OutsideTask))],
    );
    let mut w = world(
        BackendKind::CosmicStock,
        vec![reply(click()), reply(finish())],
        pdp,
    );
    let report = block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).expect("ends");
    assert_eq!(outcome_of(&report), CuaOutcome::Done);
    let second = &w.model.requests()[1];
    assert_eq!(second.prev, vec![PrevResult::Refused("not allowed".into())]);
    assert!(
        w.seat
            .events()
            .iter()
            .all(|e| !matches!(e, SeatEvent::Performed(..))),
        "a refused click never reaches the seat"
    );
}

#[test]
fn a_refused_consent_blocks_the_run_before_any_window_is_taken() {
    let pdp = FakePdp::scripted(vec![GrantAnswer::Refused(ConfirmEnd::Refused)], vec![]);
    let mut w = world(BackendKind::CosmicStock, vec![], pdp);
    let report = block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).expect("ends");
    assert_eq!(
        outcome_of(&report),
        CuaOutcome::Blocked(BlockedOn::ConsentDenied)
    );
    assert!(w.desktop.calls().is_empty(), "no lease, no glow");
    assert_eq!(
        w.events.sent()[0].1.kind,
        MessageKind::Report {
            status: ReportStatus::Failed
        }
    );
    assert_eq!(w.gate.grant_asks().len(), 1);
}

#[test]
fn a_message_that_arrived_becomes_a_note_on_the_next_turn_and_the_run_is_told_nothing_else() {
    let mut w = world(
        BackendKind::CosmicStock,
        vec![reply(finish())],
        FakePdp::allow_all(),
    );
    w.events.deliver(InboundLine {
        id: MessageId::parse("m-5").unwrap(),
        thread: ThreadId::parse("m-5").unwrap(),
        to: Address::new(
            AgentRef::Cua {
                run: prov::RunId::parse("r-1").unwrap(),
            },
            space(),
        ),
        from: Address::new(AgentRef::User, space()),
        crossing: Crossing::Within,
        kind: MessageKind::Note,
        parts: vec![docket_core::InboundPart::Text(docket_core::Reveal::Plain(
            "use the work account".to_owned(),
        ))],
    });
    block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).expect("ends");
    assert_eq!(
        w.model.requests()[0].notes,
        vec![StepNote {
            from: NoteFrom::Person,
            text: "use the work account".to_owned()
        }]
    );
}

#[test]
fn a_fatal_model_ends_the_run_failed_and_the_lease_is_given_back() {
    let mut w = world(
        BackendKind::CosmicStock,
        vec![Err(CuaModelError::Fatal)],
        FakePdp::allow_all(),
    );
    let report = block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).expect("ends");
    assert_eq!(outcome_of(&report), CuaOutcome::Failed(FailReason::Model));
    assert!(matches!(
        w.desktop.calls().last(),
        Some(DesktopCall::Released(_))
    ));
    assert_eq!(
        report.signals.last(),
        Some(&CuaSignal::Finished(outcome_of(&report)))
    );
}

#[test]
fn a_model_that_runs_dry_is_a_failed_run_not_a_hang() {
    let mut w = world(BackendKind::CosmicStock, vec![], FakePdp::allow_all());
    let report = block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).expect("ends");
    assert_eq!(outcome_of(&report), CuaOutcome::Failed(FailReason::Model));
}

#[test]
fn a_question_nobody_answers_is_stuck_not_silently_finished() {
    let ask = CuaAction::Ask {
        question: cua_action::Summary::new("Which one?").unwrap(),
        choices: vec![],
    };
    let mut w = world(
        BackendKind::CosmicStock,
        vec![reply(ask)],
        FakePdp::allow_all(),
    );
    let err = block_on(run_harness(started(BackendKind::CosmicStock), &mut w)).unwrap_err();
    assert!(
        matches!(
            err,
            cua_fake::HarnessError::Stuck(RunPhase::WaitingForUser(..))
        ),
        "{err:?}"
    );
}
