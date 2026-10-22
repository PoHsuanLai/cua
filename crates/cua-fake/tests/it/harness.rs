//! The end-to-end scripted run over the fakes: click, then done.

use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, FinishOutcome, PixelFormat, Point, RunMode,
    Scale120, Size, Summary, Target, WindowSpace, WindowTrust,
};
use cua_bus::{
    CuaOutcome, CuaTask, ProcedureUse, ReportRoute, RunBudget, RunOrigin, SuccessCheck, TaskTarget,
};
use cua_desktop::{BackendCaps, BackendKind, MaskWhere, Stamp, ToplevelIdent};
use cua_fake::{
    DesktopCall, FakeA11y, FakeCapture, FakeDesktop, FakePdp, FakeSeat, FixedClock, Fixture,
    MemoryEvents, ScriptedModel, SeatEvent, World, run_harness_answering,
};
use cua_run::{
    AcquirePhase, CuaConfig, CuaSignal, FORCED_YES, Progress, Run, RunEnv, RunPhase, Step,
};
use maskcap::{Frame, FrameSeq, MaskReport, PixelBuf, WholeMask};
use porter_core::{AppName, Px, UnixSeconds};
use porter_infer::CuaStepReply;
use prov::{
    Address, AgentRef, Confidentiality, Integrity, Label, Labelled, MessageKind, ReportStatus,
    RunId, SessionId, Source, SpaceId,
};
use std::collections::BTreeSet;
use std::sync::Arc;

fn space() -> SpaceId {
    SpaceId::parse("desktop").unwrap()
}

fn app() -> AppName {
    AppName::parse("org.mozilla.firefox").unwrap()
}

fn frame(seq: u64) -> Frame {
    Frame {
        seq: FrameSeq(seq),
        at: Stamp(seq * 100),
        pixels: PixelBuf {
            format: PixelFormat::Xbgr8888,
            size: DeviceSize { w: 100, h: 80 },
            stride: 400,
            data: Arc::from(vec![seq as u8; 400 * 80]),
        },
        logical: Size::new(Coord(100), Coord(80)),
        scale: Scale120(120),
        damage: vec![],
        masks: MaskReport {
            applied_by: MaskWhere::Cuad,
            whole: WholeMask::None,
            regions: vec![],
        },
    }
}

fn window() -> cua_desktop::ToplevelInfo {
    cua_desktop::ToplevelInfo {
        ident: ToplevelIdent::parse("w1").unwrap(),
        app: app(),
        title: cua_desktop::WindowTitle("Sign in".into()),
        pid: cua_desktop::Pid(77),
        geometry: cua_desktop::GlobalRect {
            x: cua_desktop::GlobalCoord(0),
            y: cua_desktop::GlobalCoord(0),
            w: Px(100),
            h: Px(80),
        },
        workspace: cua_desktop::WorkspaceRef {
            id: "ws-1".into(),
            shown: cua_desktop::WorkspaceShown::Shown,
        },
        state: BTreeSet::from([cua_desktop::ToplevelState::Activated]),
        trust: WindowTrust::Native,
        parent: None,
    }
}

fn reply(action: CuaAction<WindowSpace>) -> CuaStepReply {
    CuaStepReply::new(vec![action])
}

fn user_goal() -> Labelled<String> {
    Labelled::new(
        "sign in".to_owned(),
        Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Public,
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::User]),
        },
    )
}

fn new_world(script: Vec<Result<CuaStepReply, cua_run::CuaModelError>>) -> World {
    World {
        desktop: FakeDesktop::new(BackendKind::KwinStock, vec![window()]),
        seat: FakeSeat::new(cua_desktop::SeatIsolation::SharedSeat, Stamp(0)),
        capture: FakeCapture::scripted(vec![frame(1), frame(2), frame(3)], vec![]),
        a11y: FakeA11y::fixture(Fixture::FirefoxForm),
        model: ScriptedModel::new(script),
        gate: FakePdp::allow_all(),
        events: MemoryEvents::new(),
        clock: FixedClock::at(Stamp(0), UnixSeconds(1_700_000_000)),
    }
}

fn new_run() -> Run {
    let task = CuaTask {
        app: app(),
        goal: user_goal(),
        asked: None,
        earlier: None,
        target: TaskTarget::Window(ToplevelIdent::parse("w1").unwrap()),
        mode: RunMode::InPlace,
        space: space(),
        session: SessionId::parse("s-1").unwrap(),
        conversation: SessionId::parse("s-1").unwrap(),
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
    };
    let env = RunEnv::new(
        &CuaConfig::default(),
        BackendCaps::of(BackendKind::KwinStock),
    );
    Run::start(RunId::parse("r-1").unwrap(), task, env, Stamp(0)).0
}

#[test]
fn end_to_end_scripted_run_done() {
    let click = CuaAction::Click {
        at: Target::Point(Point::new(Coord(10), Coord(10))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    };
    let finish = CuaAction::Finish {
        outcome: FinishOutcome::Done,
        summary: Summary::new("Signed in.").unwrap(),
        extracted: vec![],
    };
    let mut world = new_world(vec![Ok(reply(click)), Ok(reply(finish))]);
    let run = new_run();

    // The fixture is a sign-in form with a password field: a sensitive window, so the run asks
    // before the click whatever the gate says, and the scripted person says yes.
    let report = block_on(run_harness_answering(
        run,
        &mut world,
        vec![FORCED_YES.to_owned()],
    ))
    .expect("the run ends");

    assert_eq!(report.run.phase, RunPhase::Finished(CuaOutcome::Done));
    assert_eq!(
        report.signals.last(),
        Some(&CuaSignal::Finished(CuaOutcome::Done))
    );
    assert!(
        report
            .signals
            .iter()
            .any(|s| matches!(s, CuaSignal::ActionProposed(..)))
    );
    assert!(
        report
            .signals
            .iter()
            .any(|s| matches!(s, CuaSignal::ActionDone(..)))
    );

    let asked = report
        .signals
        .iter()
        .position(|s| matches!(s, CuaSignal::AskUser(..)))
        .expect("the sensitive window forced a question");
    let clicked = report
        .signals
        .iter()
        .position(|s| matches!(s, CuaSignal::ActionDone(..)))
        .expect("the click ran");
    assert!(asked < clicked, "the question comes before the click");

    let kinds: Vec<String> = world.events.records().iter().map(|r| r.kind()).collect();
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

    let performed = world
        .seat
        .events()
        .into_iter()
        .filter(|e| matches!(e, SeatEvent::Performed(..)))
        .count();
    assert_eq!(performed, 1, "one click reached the seat");
    let calls = world.desktop.calls();
    assert!(matches!(calls.first(), Some(DesktopCall::Leased(_))));
    assert!(
        matches!(calls.last(), Some(DesktopCall::Released(_))),
        "the lease is released"
    );

    let sent = world.events.sent();
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
}

/// A minimal executor for futures that never wait.
fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}

/// A step of taking the window that stalls ends the run blocked in words that name it, the lease
/// is given back, and the log has the step's start and its timeout.
fn stalled(phase: AcquirePhase) -> (cua_fake::HarnessReport, World) {
    let mut world = new_world(vec![]);
    world.clock = FixedClock::at(Stamp(0), UnixSeconds(1_700_000_000)).impatient();
    match phase {
        AcquirePhase::Attach => world.seat.stall_next_attach(),
        AcquirePhase::Capture => world.capture.stall_next_open(),
    }
    let report = block_on(run_harness_answering(new_run(), &mut world, vec![])).expect("ends");
    (report, world)
}

#[test]
fn a_stalled_step_of_taking_the_window_names_itself_and_gives_the_lease_back() {
    let table = [
        (AcquirePhase::Attach, "its input never answered", "attach"),
        (
            AcquirePhase::Capture,
            "its picture never arrived",
            "capture",
        ),
    ];
    for (phase, words, name) in table {
        let (report, world) = stalled(phase);
        assert_eq!(
            report.run.phase,
            RunPhase::Finished(CuaOutcome::Blocked(cua_bus::BlockedOn::LeaseRefused)),
            "{name}"
        );
        let sent: Vec<String> = world
            .events
            .sent()
            .into_iter()
            .flat_map(|(_, draft)| draft.parts)
            .filter_map(|part| match part {
                docket_core::DraftPart::Text(text) => Some(text.as_str().to_owned()),
                _ => None,
            })
            .collect();
        assert!(sent.iter().any(|t| t.contains(words)), "{name}: {sent:?}");
        assert!(
            matches!(world.desktop.calls().last(), Some(DesktopCall::Released(_))),
            "{name}: the lease is ended"
        );
        assert!(
            matches!(world.seat.events().last(), Some(SeatEvent::Detached(_))),
            "{name}: the input is let go"
        );
        let steps: Vec<(Step, bool)> = report
            .notes
            .iter()
            .map(|n| (n.step, matches!(n.progress, Progress::Failed(..))))
            .collect();
        let named = Step::Phase(phase);
        assert!(steps.contains(&(Step::Lease, false)), "{name}: {steps:?}");
        assert!(steps.contains(&(named, false)), "{name}: {steps:?}");
        assert!(steps.contains(&(named, true)), "{name}: {steps:?}");
    }
}

/// The run on a window without the focus, on a desktop of `kind`: what reached the seat, the
/// steps' results and what the desktop was asked.
fn on_an_unfocused_window(
    kind: BackendKind,
    first: CuaAction<WindowSpace>,
) -> (usize, Vec<String>, Vec<DesktopCall>) {
    let finish = CuaAction::Finish {
        outcome: FinishOutcome::Done,
        summary: Summary::new("Signed in.").unwrap(),
        extracted: vec![],
    };
    let mut world = new_world(vec![Ok(reply(first)), Ok(reply(finish))]);
    let mut unfocused = window();
    unfocused.state = BTreeSet::new();
    world.desktop = FakeDesktop::new(kind, vec![unfocused]);
    world.seat = FakeSeat::new(cua_desktop::SeatIsolation::SharedSeat, Stamp(0)).with_us_keymap();
    world.clock = FixedClock::at(Stamp(0), UnixSeconds(1_700_000_000)).impatient();
    let report = block_on(run_harness_answering(
        new_run(),
        &mut world,
        vec![FORCED_YES.to_owned()],
    ))
    .expect("the run ends");
    assert_eq!(report.run.phase, RunPhase::Finished(CuaOutcome::Done));
    let performed = world
        .seat
        .events()
        .into_iter()
        .filter(|e| matches!(e, SeatEvent::Performed(..)))
        .count();
    let steps = world
        .events
        .records()
        .into_iter()
        .filter(|r| r.kind() == "cua.step")
        .map(|r| format!("{r:?}"))
        .collect();
    (performed, steps, world.desktop.calls())
}

/// On the person's seat keys wait a moment for their window to have the focus; when it never
/// comes they are not sent and the model is told the window is not focused. A click needs no
/// focus (it is how a window gets it) and goes.
#[test]
fn keys_need_the_focus_on_the_persons_seat_and_a_click_does_not() {
    let (performed, steps, calls) =
        on_an_unfocused_window(BackendKind::KwinStock, cua_fake::scene::ctrl_a());
    assert_eq!(performed, 0, "no key reached a window without the focus");
    assert!(
        !calls.iter().any(|c| matches!(c, DesktopCall::Focused(_))),
        "a backend that cannot focus a window is not asked to"
    );
    assert!(
        steps.iter().any(|s| s.contains("NotFocused")),
        "refused as not focused: {steps:?}"
    );

    let click = CuaAction::Click {
        at: Target::Point(Point::new(Coord(10), Coord(10))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    };
    let (performed, ..) = on_an_unfocused_window(BackendKind::KwinStock, click);
    assert_eq!(performed, 1, "the click went");
}

/// On casement the run asks for the focus of its leased window before keys, instead of waiting
/// for a focus that never comes: the keys land, and a click (which needs no focus) asks nothing.
#[test]
fn keys_for_an_unfocused_leased_window_ask_casement_to_focus_it_and_land() {
    let (performed, steps, calls) =
        on_an_unfocused_window(BackendKind::Casement, cua_fake::scene::ctrl_a());
    assert!(
        calls.iter().any(|c| matches!(c, DesktopCall::Focused(_))),
        "the window was asked to take the focus: {calls:?}"
    );
    assert!(
        !steps.iter().any(|s| s.contains("NotFocused")),
        "not refused: {steps:?}"
    );
    assert_eq!(performed, 1, "the keys landed: {steps:?}");
}

/// A dialog the leased window opened (a file chooser) is what the run looks at and clicks in:
/// the capture opens a session on it and the click is aimed in its frame. Another window of the
/// app is not followed.
#[test]
fn the_run_looks_at_and_clicks_in_the_dialog_its_window_opened() {
    let click = CuaAction::Click {
        at: Target::Point(Point::new(Coord(10), Coord(10))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    };
    let finish = CuaAction::Finish {
        outcome: FinishOutcome::Done,
        summary: Summary::new("Opened.").unwrap(),
        extracted: vec![],
    };
    let mut world = new_world(vec![Ok(reply(click)), Ok(reply(finish))]);
    let mut dialog = window();
    dialog.ident = ToplevelIdent::parse("d1").unwrap();
    dialog.title = cua_desktop::WindowTitle("Open File".into());
    dialog.geometry.x = cua_desktop::GlobalCoord(200);
    dialog.geometry.y = cua_desktop::GlobalCoord(100);
    dialog.parent = Some(ToplevelIdent::parse("w1").unwrap());
    let mut other = window();
    other.ident = ToplevelIdent::parse("w2").unwrap();
    let mut behind = window();
    behind.state = BTreeSet::new();
    world.desktop = FakeDesktop::new(BackendKind::Casement, vec![behind, dialog.clone(), other]);
    let report = block_on(run_harness_answering(
        new_run(),
        &mut world,
        vec![FORCED_YES.to_owned()],
    ))
    .expect("the run ends");

    assert_eq!(report.run.phase, RunPhase::Finished(CuaOutcome::Done));
    let opened: Vec<String> = world
        .capture
        .opened()
        .iter()
        .map(|w| w.as_str().to_owned())
        .collect();
    assert_eq!(opened, ["w1", "d1"], "the leased window, then its dialog");
    let performed = world
        .seat
        .events()
        .into_iter()
        .filter(|e| matches!(e, SeatEvent::Performed(..)))
        .count();
    assert_eq!(performed, 1, "the click went");
    assert_eq!(
        world.seat.rebinds().last().map(|(_, frame)| *frame),
        Some(dialog.geometry),
        "aimed in the dialog's frame"
    );
}
