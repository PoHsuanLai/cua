//! The run machine's transitions, as the phase table of cua.md section 4 (changed for the
//! router's gate) states them.

use cua_action::{Button, ClickCount, Coord, CuaAction, Point, Target, WindowSpace};
use cua_bus::{
    BlockedOn, BudgetUse, CancelledBy, CuaOutcome, CuaTask, ProcedureUse, ReportRoute, RunBudget,
    RunOrigin, SuccessCheck, TargetedAction, TaskTarget, WindowRef,
};
use cua_desktop::{
    BackendCaps, BackendKind, ConsentGuard, DesktopEvent, LeaseId, Stamp, Suspend, ToplevelIdent,
};
use cua_run::{
    ActionQueue, CuaConfig, CuaSignal, Repeats, Run, RunEffect, RunEnv, RunInput, RunPhase,
    RunTrack,
};
use docket_core::{GrantAnswer, RunMode};
use porter_core::{AppName, DataClass};
use porter_infer::StepIndex;
use prov::{
    Address, AgentRef, Confidentiality, ConfirmId, Integrity, Label, Labelled, MessageKind,
    ReportStatus, RunId, SessionId, Source, SpaceId,
};
use std::collections::BTreeSet;

fn space() -> SpaceId {
    SpaceId::parse("desktop").unwrap()
}

fn app() -> AppName {
    AppName::parse("org.mozilla.firefox").unwrap()
}

fn user_goal() -> Labelled<String> {
    Labelled::new(
        "download the March invoice".to_owned(),
        Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Public,
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::User]),
        },
    )
}

fn screen_label() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space()])),
        classes: BTreeSet::from([DataClass::Screen]),
        sources: BTreeSet::from([Source::Screen { app: app() }]),
    }
}

fn task(target: TaskTarget, mode: RunMode) -> CuaTask {
    CuaTask {
        app: app(),
        goal: user_goal(),
        target,
        mode,
        space: space(),
        session: SessionId::parse("s-1").unwrap(),
        budget: RunBudget::PROPOSED,
        data_in: vec![],
        success: SuccessCheck::ModelSays,
        origin: RunOrigin::Planner,
        procedure: ProcedureUse::None,
        report: ReportRoute {
            to: Address::new(AgentRef::Companion, space()),
            thread: None,
            in_reply_to: None,
        },
    }
}

fn window() -> TaskTarget {
    TaskTarget::Window(ToplevelIdent::parse("w1").unwrap())
}

fn env(kind: BackendKind) -> RunEnv {
    RunEnv::new(&CuaConfig::default(), BackendCaps::of(kind))
}

fn run_id() -> RunId {
    RunId::parse("r-1").unwrap()
}

/// A run already in `phase`, holding lease 1, as the machine would have left it.
fn in_phase(phase: RunPhase, kind: BackendKind) -> Run {
    Run {
        id: run_id(),
        task: task(window(), RunMode::InPlace),
        env: env(kind),
        phase,
        used: BudgetUse::NONE,
        taint: screen_label(),
        frames: Repeats::default(),
        pending: ActionQueue::default(),
        lease: Some(LeaseId(1)),
        served: None,
        working: None,
        last_at: Stamp(0),
        track: RunTrack::default(),
    }
}

fn click() -> TargetedAction {
    TargetedAction {
        window: WindowRef(1),
        action: CuaAction::<WindowSpace>::Click {
            at: Target::Point(Point::new(Coord(10), Coord(10))),
            button: Button::Left,
            count: ClickCount::One,
            mods: BTreeSet::new(),
        },
    }
}

fn report_status(effects: &[RunEffect]) -> Option<ReportStatus> {
    effects.iter().find_map(|e| match e {
        RunEffect::Report(draft) => match draft.kind {
            MessageKind::Report { status } => Some(status),
            MessageKind::Note | MessageKind::Request => None,
        },
        _ => None,
    })
}

fn finished_signal(effects: &[RunEffect]) -> Option<CuaOutcome> {
    effects.iter().find_map(|e| match e {
        RunEffect::Emit(CuaSignal::Finished(outcome)) => Some(*outcome),
        _ => None,
    })
}

#[test]
fn a_started_run_asks_for_the_consent_first() {
    let (run, effects) = Run::start(
        run_id(),
        task(window(), RunMode::InPlace),
        env(BackendKind::CosmicStock),
        Stamp(0),
    );
    assert_eq!(run.phase, RunPhase::Granting);
    assert_eq!(
        effects,
        vec![RunEffect::Grant {
            app: app(),
            space: space()
        }]
    );
}

#[test]
fn a_mode_the_backend_cannot_run_or_an_app_target_ends_the_run_at_once() {
    let (run, effects) = Run::start(
        run_id(),
        task(window(), RunMode::AgentWorkspace),
        env(BackendKind::CosmicStock),
        Stamp(0),
    );
    assert_eq!(
        run.phase,
        RunPhase::Finished(CuaOutcome::Blocked(BlockedOn::ModeUnsupported(
            RunMode::AgentWorkspace
        )))
    );
    assert_eq!(report_status(&effects), Some(ReportStatus::Failed));
    let (run, _) = Run::start(
        run_id(),
        task(TaskTarget::App(app()), RunMode::InPlace),
        env(BackendKind::CosmicStock),
        Stamp(0),
    );
    assert_eq!(
        run.phase,
        RunPhase::Finished(CuaOutcome::Blocked(BlockedOn::NeedsCapability))
    );
}

#[test]
fn a_refused_consent_ends_the_run_blocked_and_reports_failed() {
    let (run, _) = Run::start(
        run_id(),
        task(window(), RunMode::InPlace),
        env(BackendKind::CosmicStock),
        Stamp(0),
    );
    let refused = GrantAnswer::Refused(docket_core::ConfirmEnd::Refused);
    let (run, effects) = run.step(RunInput::Granted(refused), Stamp(10));
    assert_eq!(
        run.phase,
        RunPhase::Finished(CuaOutcome::Blocked(BlockedOn::ConsentDenied))
    );
    assert_eq!(
        finished_signal(&effects),
        Some(CuaOutcome::Blocked(BlockedOn::ConsentDenied))
    );
    assert_eq!(report_status(&effects), Some(ReportStatus::Failed));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, RunEffect::ReleaseLease(_))),
        "no lease was taken"
    );
}

#[test]
fn finished_ignores_every_input() {
    let finished = RunPhase::Finished(CuaOutcome::Cancelled(CancelledBy::User));
    let inputs = [
        RunInput::UserPause,
        RunInput::UserResume,
        RunInput::UserCancel,
        RunInput::UserTakeOver,
        RunInput::Tick,
        RunInput::Desktop(DesktopEvent::KillChord),
        RunInput::Confirming(ConfirmId::parse("c-1").unwrap()),
    ];
    for input in inputs {
        let before = in_phase(finished.clone(), BackendKind::CosmicStock);
        let (after, effects) = before.clone().step(input.clone(), Stamp(99));
        assert_eq!(after, before, "{input:?}");
        assert!(effects.is_empty(), "{input:?}");
    }
}

#[test]
fn a_confirmation_suspends_the_lease_on_stock_only() {
    let confirm = ConfirmId::parse("c-1").unwrap();
    let n = StepIndex(2);
    let cases = [(
        BackendKind::CosmicStock,
        ConsentGuard::CuadSuspendsInput,
        true,
    )];
    for (kind, guard, suspends) in cases {
        assert_eq!(BackendCaps::of(kind).consent, guard);
        let run = in_phase(RunPhase::Authorising(n), kind);
        let (run, effects) = run.step(RunInput::Confirming(confirm.clone()), Stamp(5));
        assert_eq!(
            run.phase,
            RunPhase::Confirming(n, confirm.clone()),
            "{kind:?}"
        );
        let suspend = effects
            .iter()
            .position(|e| matches!(e, RunEffect::SuspendLease(LeaseId(1), Suspend::Consent)));
        let proceed = effects.iter().position(|e| matches!(e, RunEffect::Proceed));
        assert_eq!(suspend.is_some(), suspends, "{kind:?}");
        assert!(proceed.is_some(), "{kind:?}: Proceed always follows");
        if let (Some(s), Some(p)) = (suspend, proceed) {
            assert!(s < p, "input stops before the router shows anything");
        }
    }
}

#[test]
fn pause_drops_the_queue_and_resume_observes_afresh() {
    let n = StepIndex(2);
    let mut run = in_phase(RunPhase::Authorising(n), BackendKind::CosmicStock);
    run.pending = ActionQueue {
        actions: vec![click(), click(), click()].into(),
    };
    let (run, effects) = run.step(RunInput::UserPause, Stamp(7));
    assert_eq!(run.phase, RunPhase::Paused(n));
    assert!(run.pending.actions.is_empty());
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, RunEffect::SuspendLease(_, Suspend::Paused)))
    );
    let (run, effects) = run.step(RunInput::UserResume, Stamp(9));
    assert_eq!(run.phase, RunPhase::Observing(StepIndex(3)));
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, RunEffect::Capture { .. }))
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, RunEffect::ResumeLease(LeaseId(1))))
    );
}

#[test]
fn a_message_changes_no_authority() {
    let n = StepIndex(2);
    let before = in_phase(RunPhase::Observing(n), BackendKind::CosmicStock);
    let message = docket_core::InboundLine {
        id: prov::MessageId::parse("m-1").unwrap(),
        thread: prov::ThreadId::parse("m-1").unwrap(),
        from: Address::new(AgentRef::User, space()),
        to: Address::new(AgentRef::Cua { run: run_id() }, space()),
        crossing: prov::Crossing::Within,
        kind: MessageKind::Request,
        parts: vec![],
    };
    let (after, effects) = before
        .clone()
        .step(RunInput::Inbound(Box::new(message)), Stamp(3));
    assert_eq!(after.phase, before.phase);
    assert_eq!(after.lease, before.lease);
    assert_eq!(
        after.task, before.task,
        "no message widens the mode, the budget or the goal"
    );
    assert_eq!(after.used, before.used);
    assert!(
        !effects.iter().any(|e| matches!(
            e,
            RunEffect::AcquireLease(_)
                | RunEffect::Inject(_)
                | RunEffect::Check(_)
                | RunEffect::Grant { .. }
        )),
        "a message starts nothing: {effects:?}"
    );
}
