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
use cua_run::{CuaConfig, CuaSignal, FORCED_YES, Run, RunEnv, RunPhase};
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
        state: BTreeSet::new(),
        trust: WindowTrust::Native,
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
    let mut world = World {
        desktop: FakeDesktop::new(BackendKind::KwinStock, vec![window()]),
        seat: FakeSeat::new(cua_desktop::SeatIsolation::SharedSeat, Stamp(0)),
        capture: FakeCapture::scripted(vec![frame(1), frame(2), frame(3)], vec![]),
        a11y: FakeA11y::fixture(Fixture::FirefoxForm),
        model: ScriptedModel::new(vec![Ok(reply(click)), Ok(reply(finish))]),
        gate: FakePdp::allow_all(),
        events: MemoryEvents::new(),
        clock: FixedClock::at(Stamp(0), UnixSeconds(1_700_000_000)),
    };
    let task = CuaTask {
        app: app(),
        goal: user_goal(),
        target: TaskTarget::Window(ToplevelIdent::parse("w1").unwrap()),
        mode: RunMode::InPlace,
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
    };
    let env = RunEnv::new(
        &CuaConfig::default(),
        BackendCaps::of(BackendKind::KwinStock),
    );
    let (run, _) = Run::start(RunId::parse("r-1").unwrap(), task, env, Stamp(0));

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
