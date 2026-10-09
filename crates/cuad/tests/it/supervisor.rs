//! The supervisor over the fakes: a run end to end, who may start and control, the lease book,
//! and what the person's controls do to a run in flight.

use crate::support::*;
use cua_action::{CuaAction, Summary};
use cua_bus::{
    CancelledBy, ControlRefusal, CuaCaller, CuaMember, CuaOutcome, QuestionId, ReportDelivery,
};
use cua_desktop::{BackendKind, DesktopEvent, Suspend};
use cua_fake::scene::{click, finish, reply, task, user_goal, window};
use cua_fake::{DesktopCall, FakePdp, Fixture, SeatEvent, Verdict};
use cua_run::{CuaSignal, FORCED_YES, RunPhase};
use cuad::Supervisor;
use docket_core::GateAnswer;
use prov::{Labelled, MessageKind, ReportStatus};
use seat_input::Emulation;

const STOCK: BackendKind = BackendKind::CosmicStock;

fn ask() -> CuaAction<cua_action::WindowSpace> {
    CuaAction::Ask {
        question: Summary::new("Which account?").unwrap(),
        choices: vec![],
    }
}

#[tokio::test(start_paused = true)]
async fn end_to_end_scripted_run_done() {
    let sup = supervisor(
        STOCK,
        vec![reply(click()), reply(finish())],
        FakePdp::allow_all(),
    );
    let mut events = sup.subscribe();
    let id = sup
        .start(CuaCaller::Companion, task())
        .await
        .expect("started");
    assert_eq!(id.as_str(), "r-1");
    // The fixture window is a sign-in form with a password field: the run asks before the
    // click whatever the gate says, and the person says yes.
    until(&sup, &id, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    sup.answer(
        CuaCaller::Shell,
        &id,
        QuestionId(1),
        Labelled::new(FORCED_YES.to_owned(), user_goal().label),
    )
    .await
    .unwrap();
    let (end, signals) = ended_with(&mut events, &id).await;
    assert_eq!(end.outcome, CuaOutcome::Done);
    assert_eq!(end.steps, porter_core::Count(2));
    assert!(
        matches!(end.report, ReportDelivery::Sent(_)),
        "the report went out"
    );
    assert!(matches!(
        signals.last(),
        Some(CuaSignal::Finished(CuaOutcome::Done))
    ));
    assert!(
        signals
            .iter()
            .any(|s| matches!(s, CuaSignal::ActionProposed(..)))
    );
    assert!(
        signals
            .iter()
            .any(|s| matches!(s, CuaSignal::ActionDone(..)))
    );

    let kinds: Vec<String> = sup.events().records().iter().map(|r| r.kind()).collect();
    assert_eq!(
        kinds,
        [
            "cua.run.started",
            "cua.asked",
            "cua.confirmed",
            "cua.step",
            "cua.run.finished"
        ]
    );
    let performed = sup
        .seat()
        .events()
        .iter()
        .filter(|e| matches!(e, SeatEvent::Performed(..)))
        .count();
    assert_eq!(performed, 1, "one click reached the seat");
    assert!(matches!(
        sup.desktop().calls().last(),
        Some(DesktopCall::Released(_))
    ));
    let sent = sup.events().sent();
    assert_eq!(
        sent.len(),
        2,
        "a progress report for the question, then the final one"
    );
    assert_eq!(
        sent[1].1.kind,
        MessageKind::Report {
            status: ReportStatus::Done
        }
    );
    assert!(sup.runs().is_empty());
    assert_eq!(sup.lease_book(), cua_run::LeaseBook::default());
    assert_eq!(sup.started(), 1);
}

#[tokio::test(start_paused = true)]
async fn start_refused_for_non_planner_caller_and_while_disabled_or_unaimed() {
    let sup = supervisor(STOCK, vec![], FakePdp::allow_all());
    assert_eq!(
        sup.start(CuaCaller::Shell, task()).await,
        Err(ControlRefusal::NotPermitted)
    );
    assert_eq!(
        sup.start(CuaCaller::Other, task()).await,
        Err(ControlRefusal::NotPermitted)
    );
    let mut missing = task();
    missing.target =
        cua_bus::TaskTarget::Window(cua_desktop::ToplevelIdent::parse("nope").unwrap());
    assert_eq!(
        sup.start(CuaCaller::Router, missing).await,
        Err(ControlRefusal::WindowUnknown)
    );
    let mut hidden = task();
    hidden.mode = docket_core::RunMode::AgentWorkspace;
    assert_eq!(
        sup.start(CuaCaller::Router, hidden).await,
        Err(ControlRefusal::ModeUnsupported),
        "stock cosmic-comp cannot run the agent workspace"
    );
    assert_eq!(
        sup.started(),
        0,
        "a refused start uses no id and holds no window"
    );
    assert_eq!(sup.lease_book(), cua_run::LeaseBook::default());

    let off = Supervisor::new(
        cua_fake::scene::world(STOCK, vec![], FakePdp::allow_all()),
        cuad::CuadConfig::default(),
    );
    assert_eq!(
        off.start(CuaCaller::Router, task()).await,
        Err(ControlRefusal::Disabled)
    );
}

#[tokio::test(start_paused = true)]
async fn a_window_has_one_run_at_a_time() {
    let sup = supervisor(
        STOCK,
        vec![reply(ask()), reply(finish())],
        FakePdp::allow_all(),
    );
    let mut events = sup.subscribe();
    let first = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &first, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    assert_eq!(
        sup.start(CuaCaller::Router, task()).await,
        Err(ControlRefusal::LeaseHeld)
    );
    assert_eq!(sup.runs(), vec![first.clone()]);
    let book = sup.lease_book();
    assert_eq!(
        book.holder(&cua_desktop::ToplevelIdent::parse("w1").unwrap()),
        Some(&first)
    );
    sup.control(CuaCaller::Shell, &first, CuaMember::Cancel)
        .await
        .unwrap();
    ended(&mut events, &first).await;
    let second = sup.start(CuaCaller::Router, task()).await;
    assert_eq!(second.unwrap().as_str(), "r-2", "the window is free again");
}

#[tokio::test(start_paused = true)]
async fn the_person_answers_a_question_and_the_run_goes_on() {
    let sup = supervisor(
        STOCK,
        vec![reply(ask()), reply(finish())],
        FakePdp::allow_all(),
    );
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    let answer = || Labelled::new("the work one".to_owned(), user_goal().label);
    assert_eq!(
        sup.answer(CuaCaller::Companion, &id, QuestionId(1), answer())
            .await,
        Err(ControlRefusal::NotPermitted),
        "only the shell answers"
    );
    assert_eq!(
        sup.answer(CuaCaller::Shell, &id, QuestionId(7), answer())
            .await,
        Err(ControlRefusal::UnknownQuestion)
    );
    sup.answer(CuaCaller::Shell, &id, QuestionId(1), answer())
        .await
        .unwrap();
    let end = ended(&mut events, &id).await;
    assert_eq!(end.outcome, CuaOutcome::Done);
    let notes = &sup.events().sent();
    assert_eq!(
        notes.iter().map(|(_, d)| d.kind).collect::<Vec<_>>(),
        vec![
            MessageKind::Report {
                status: ReportStatus::Progress
            },
            MessageKind::Report {
                status: ReportStatus::Done
            }
        ],
        "the question was reported as progress before the end"
    );
}

#[tokio::test(start_paused = true)]
async fn pause_resume_take_over_and_hand_back_move_a_run_through_its_phases() {
    let sup = supervisor(
        STOCK,
        vec![reply(ask()), reply(ask()), reply(finish())],
        FakePdp::allow_all(),
    );
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::WaitingForUser(..))).await;

    let wrong = |member| sup.control(CuaCaller::Shell, &id, member);
    assert_eq!(
        wrong(CuaMember::Resume).await,
        Err(ControlRefusal::WrongState)
    );
    assert_eq!(
        wrong(CuaMember::HandBack).await,
        Err(ControlRefusal::WrongState)
    );
    assert_eq!(
        sup.control(CuaCaller::Other, &id, CuaMember::Pause).await,
        Err(ControlRefusal::NotPermitted)
    );

    sup.control(CuaCaller::Shell, &id, CuaMember::Pause)
        .await
        .unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::Paused(_))).await;
    assert_eq!(
        wrong(CuaMember::Pause).await,
        Err(ControlRefusal::WrongState)
    );
    assert_eq!(
        sup.lease_book()
            .slots
            .values()
            .map(|slot| slot.state)
            .collect::<Vec<_>>(),
        vec![cua_run::SlotState::Suspended],
        "a paused run still holds its window, with input stopped"
    );
    assert!(
        sup.desktop()
            .calls()
            .iter()
            .any(|c| matches!(c, DesktopCall::Suspended(_, Suspend::Paused)))
    );

    sup.control(CuaCaller::Shell, &id, CuaMember::Resume)
        .await
        .unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    sup.control(CuaCaller::Companion, &id, CuaMember::TakeOver)
        .await
        .unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::TakenOver(..))).await;
    sup.control(CuaCaller::Router, &id, CuaMember::HandBack)
        .await
        .unwrap();
    let end = ended(&mut events, &id).await;
    assert_eq!(end.outcome, CuaOutcome::Done, "the third reply finishes it");
    let kinds: Vec<String> = sup.events().records().iter().map(|r| r.kind()).collect();
    assert!(kinds.contains(&"cua.taken_over".to_owned()), "{kinds:?}");
    assert!(kinds.contains(&"cua.handed_back".to_owned()), "{kinds:?}");
}

#[tokio::test(start_paused = true)]
async fn cancel_ends_a_run_whatever_it_is_waiting_on() {
    // A run waiting for the person.
    let sup = supervisor(STOCK, vec![reply(ask())], FakePdp::allow_all());
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    sup.control(CuaCaller::Shell, &id, CuaMember::Cancel)
        .await
        .unwrap();
    let end = ended(&mut events, &id).await;
    assert_eq!(end.outcome, CuaOutcome::Cancelled(CancelledBy::User));
    assert_eq!(
        sup.events().sent().last().unwrap().1.kind,
        MessageKind::Report {
            status: ReportStatus::Cancelled
        }
    );
    assert_eq!(
        sup.control(CuaCaller::Shell, &id, CuaMember::Cancel).await,
        Err(ControlRefusal::WrongState),
        "a run that is over is not there to cancel"
    );
}

#[tokio::test(start_paused = true)]
async fn kill_chord_finishes_all_runs() {
    let mut w = cua_fake::scene::world(STOCK, vec![reply(ask())], FakePdp::allow_all());
    let mut second = window();
    second.ident = cua_desktop::ToplevelIdent::parse("w2").unwrap();
    w.desktop = cua_fake::FakeDesktop::new(STOCK, vec![window(), second]);
    let sup = Supervisor::new(w, enabled());
    let mut events = sup.subscribe();
    let first = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &first, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    let mut other = task();
    other.target = cua_bus::TaskTarget::Window(cua_desktop::ToplevelIdent::parse("w2").unwrap());
    let second_id = sup.start(CuaCaller::Router, other).await.unwrap();
    // The second run waits for the model, which the first still holds.
    until(&sup, &second_id, |p| matches!(p, RunPhase::Thinking(_))).await;

    sup.desktop().raise(DesktopEvent::KillChord);
    for id in [&first, &second_id] {
        let end = ended(&mut events, id).await;
        assert_eq!(
            end.outcome,
            CuaOutcome::Cancelled(CancelledBy::KillSwitch),
            "{id}"
        );
    }
    assert!(sup.runs().is_empty());
    assert_eq!(sup.lease_book(), cua_run::LeaseBook::default());
}

#[tokio::test(start_paused = true)]
async fn stop_all_is_the_shells_kill_switch() {
    let sup = supervisor(STOCK, vec![reply(ask())], FakePdp::allow_all());
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::WaitingForUser(..))).await;
    assert_eq!(
        sup.stop_all(CuaCaller::Other).await,
        Err(ControlRefusal::NotPermitted)
    );
    sup.stop_all(CuaCaller::Shell).await.unwrap();
    assert_eq!(
        ended(&mut events, &id).await.outcome,
        CuaOutcome::Cancelled(CancelledBy::KillSwitch)
    );
}

#[tokio::test(start_paused = true)]
async fn stock_consent_guard_injects_nothing_while_prompt_shown() {
    let pdp = FakePdp::scripted(vec![], vec![Verdict::AskThen(GateAnswer::Run)]);
    let sup = supervisor(STOCK, vec![reply(click()), reply(finish())], pdp);
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    assert_eq!(ended(&mut events, &id).await.outcome, CuaOutcome::Done);
    let seat: Vec<&'static str> = sup
        .seat()
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
        ["attach", "stopped", "running", "performed", "detach"]
    );
}

#[tokio::test(start_paused = true)]
async fn the_run_acts_in_the_session_it_opened_before_its_first_check() {
    let session = prov::SessionId::parse("s-9").unwrap();
    let pdp = FakePdp::allow_all().opening(session.clone());
    // This test is about the session, not about sensitive windows: a dialog without a password
    // field, so that the run does not stop to ask before the click.
    let sup = supervisor_over(
        STOCK,
        vec![reply(click()), reply(finish())],
        pdp,
        Fixture::Gtk4Dialog,
    );
    let mut events = sup.subscribe();
    let mut planned = task();
    planned.session = prov::SessionId::parse("s-1").unwrap();
    let id = sup.start(CuaCaller::Router, planned).await.unwrap();
    assert_eq!(
        sup.view(&id).expect("live").task.session,
        session,
        "the planner's session is not the run's"
    );
    assert_eq!(ended(&mut events, &id).await.outcome, CuaOutcome::Done);
    assert_eq!(sup.gate().opened(), vec![(id.clone(), task().space)]);
    assert_eq!(sup.gate().checks()[0].run, id);
    let sent = sup.events().sent();
    assert_eq!(
        sent[0].0, session,
        "the report goes out in the run's session"
    );
}

#[tokio::test(start_paused = true)]
async fn pausing_during_a_confirmation_takes_the_check_back() {
    let pdp = FakePdp::scripted(vec![], vec![Verdict::AskAndHang]);
    let sup = supervisor(STOCK, vec![reply(click()), reply(finish())], pdp);
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::Confirming(..))).await;
    assert_eq!(sup.gate().closes(), 0);
    sup.control(CuaCaller::Shell, &id, CuaMember::Pause)
        .await
        .unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::Paused(_))).await;
    assert_eq!(sup.gate().closes(), 1, "the sheet comes down");
}
