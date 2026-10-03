#![allow(dead_code)] // each test binary uses a subset of the shared builders
//! A world of fakes, a task, and the replies a script is made of.

use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, FinishOutcome, PixelFormat, Point, Scale120,
    Size, Summary, Target, WindowSpace,
};
use cua_bus::{CuaTask, ProcedureUse, ReportRoute, RunBudget, RunOrigin, SuccessCheck, TaskTarget};
use cua_desktop::{BackendCaps, BackendKind, MaskWhere, Stamp, ToplevelIdent, ToplevelInfo};
use cua_fake::{
    FakeA11y, FakeCapture, FakeDesktop, FakePdp, FakeSeat, FixedClock, Fixture, MemoryEvents,
    ScriptedModel, World,
};
use cua_run::{CuaConfig, CuaModelError, Run, RunEnv};
use docket_core::{RunMode, WindowTrust};
use maskcap::{Frame, FrameSeq, MaskReport, PixelBuf, WholeMask};
use porter_core::{AppName, Px, UnixSeconds};
use porter_infer::CuaStepReply;
use prov::{
    Address, AgentRef, Confidentiality, Integrity, Label, Labelled, RunId, SessionId, Source,
    SpaceId,
};
use std::collections::BTreeSet;
use std::sync::Arc;

pub fn space() -> SpaceId {
    SpaceId::parse("desktop").unwrap()
}

pub fn app() -> AppName {
    AppName::parse("org.mozilla.firefox").unwrap()
}

pub fn frame(seq: u64) -> Frame {
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

pub fn window() -> ToplevelInfo {
    ToplevelInfo {
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

pub fn reply(action: CuaAction<WindowSpace>) -> Result<CuaStepReply, CuaModelError> {
    Ok(CuaStepReply {
        thought: None,
        actions: vec![action],
        dropped: vec![],
        safety: vec![],
    })
}

pub fn click() -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: Target::Point(Point::new(Coord(10), Coord(10))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

pub fn finish() -> CuaAction<WindowSpace> {
    CuaAction::Finish {
        outcome: FinishOutcome::Done,
        summary: Summary::new("Signed in.").unwrap(),
        extracted: vec![],
    }
}

pub fn user_goal() -> Labelled<String> {
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

pub fn world(
    kind: BackendKind,
    script: Vec<Result<CuaStepReply, CuaModelError>>,
    gate: FakePdp,
) -> World {
    World {
        desktop: FakeDesktop::new(kind, vec![window()]),
        seat: FakeSeat::new(BackendCaps::of(kind).seat, Stamp(0)),
        capture: FakeCapture::scripted((1..=8).map(frame).collect(), vec![]),
        a11y: FakeA11y::fixture(Fixture::FirefoxForm),
        model: ScriptedModel::new(script),
        gate,
        events: MemoryEvents::new(),
        clock: FixedClock::at(Stamp(0), UnixSeconds(1_700_000_000)),
    }
}

pub fn task() -> CuaTask {
    CuaTask {
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
    }
}

pub fn started(kind: BackendKind) -> Run {
    let env = RunEnv::new(&CuaConfig::default(), BackendCaps::of(kind));
    Run::start(RunId::parse("r-1").unwrap(), task(), env, Stamp(0)).0
}

/// A minimal executor for futures that never wait.
pub fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
    }
}
