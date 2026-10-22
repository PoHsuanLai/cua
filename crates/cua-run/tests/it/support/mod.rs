#![allow(dead_code)] // each test binary uses a subset of the shared builders
//! Builders for driving the run machine: tasks, observations, replies and a small harness that
//! feeds inputs one at a time and keeps the effects of the last one.

use a11y_tree::{
    A11yNode, A11ySnapshot, NodeState, NodeValue, Parent, Role, Sensitivity, TreeSource,
    Truncation, TypedHint,
};
use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, FinishOutcome, PixelFormat, Point, Rect,
    Scale120, Size, Summary, Target, WaitMs, WindowSpace,
};
use cua_action::{RunMode, WindowTrust};
use cua_bus::{
    CuaTask, ProcedureUse, ReportRoute, RunBudget, RunOrigin, SuccessCheck, TaskTarget, WindowRef,
};
use cua_desktop::{
    BackendCaps, BackendKind, GlobalCoord, GlobalRect, LeaseGrant, LeaseId, MaskWhere, Pid, Stamp,
    ToplevelIdent, ToplevelInfo, WindowTitle, WorkspaceRef, WorkspaceShown,
};
use cua_run::{
    ContextObs, CuaConfig, CursorObs, Focus, ModelReply, Observation, Run, RunEffect, RunEnv,
    RunInput, RunPhase, TreeObs, Visibility, WindowInfo,
};
use maskcap::{Frame, FrameSeq, MaskReport, PixelBuf, WholeMask};
use porter_core::{AppName, Count, DataClass, MicroUsd, Px, Tokens};
use porter_infer::{CuaStepReply, StepIndex, TokenUsage};
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

pub fn user_goal() -> Labelled<String> {
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

pub fn screen_label() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space()])),
        classes: BTreeSet::from([DataClass::Screen]),
        sources: BTreeSet::from([Source::Screen { app: app() }]),
    }
}

pub fn task(success: SuccessCheck) -> CuaTask {
    CuaTask {
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
        success,
        origin: RunOrigin::Planner,
        procedure: ProcedureUse::None,
        report: ReportRoute {
            to: Address::new(AgentRef::Companion, space()),
            thread: None,
            in_reply_to: None,
        },
    }
}

pub fn env(kind: BackendKind) -> RunEnv {
    RunEnv::new(&CuaConfig::default(), BackendCaps::of(kind))
}

pub fn run_id() -> RunId {
    RunId::parse("r-1").unwrap()
}

pub fn window_info() -> ToplevelInfo {
    ToplevelInfo {
        ident: ToplevelIdent::parse("w1").unwrap(),
        app: app(),
        title: WindowTitle("Sign in".into()),
        pid: Pid(77),
        geometry: GlobalRect {
            x: GlobalCoord(0),
            y: GlobalCoord(0),
            w: Px(100),
            h: Px(80),
        },
        workspace: WorkspaceRef {
            id: "ws-1".into(),
            shown: WorkspaceShown::Shown,
        },
        state: BTreeSet::new(),
        trust: WindowTrust::Native,
        parent: None,
    }
}

pub fn grant(kind: BackendKind) -> LeaseGrant {
    LeaseGrant {
        id: LeaseId(1),
        window: window_info(),
        seat: BackendCaps::of(kind).seat,
        ei: cua_desktop::EiOrigin::SenderSocket,
    }
}

pub fn frame(byte: u8) -> Frame {
    Frame {
        seq: FrameSeq(u64::from(byte)),
        at: Stamp(0),
        pixels: PixelBuf {
            format: PixelFormat::Xbgr8888,
            size: DeviceSize { w: 100, h: 80 },
            stride: 400,
            data: Arc::from(vec![byte; 400 * 80]),
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

pub fn node(id: u32, role: Role, name: &str, bounds: (u32, u32, u32, u32)) -> A11yNode {
    A11yNode {
        id: cua_action::NodeId(id),
        parent: if id == 0 {
            Parent::Root
        } else {
            Parent::Node(cua_action::NodeId(0))
        },
        role,
        name: Labelled::new(name.to_owned(), screen_label()),
        value: NodeValue::None,
        states: BTreeSet::from([NodeState::Enabled, NodeState::Visible, NodeState::Showing]),
        bounds: Rect {
            origin: Point::new(Coord(bounds.0), Coord(bounds.1)),
            size: Size::new(Coord(bounds.2), Coord(bounds.3)),
        },
        actions: vec![],
        typed: TypedHint::None,
        sensitivity: Sensitivity::Normal,
    }
}

pub fn tree(nodes: Vec<A11yNode>) -> TreeObs {
    TreeObs::Present(A11ySnapshot {
        source: TreeSource::Atspi,
        taken: Stamp(0),
        nodes,
        truncated: Truncation::Complete,
    })
}

/// What the supervisor hands the machine when the capture and the snapshot are in: step and
/// previous outcomes are the machine's to fill.
pub fn observation(byte: u8, tree: TreeObs) -> Observation {
    Observation {
        step: StepIndex(0),
        window: WindowInfo {
            window: WindowRef(1),
            app: app(),
            title: Labelled::new("Sign in".to_owned(), screen_label()),
            size: Size::new(Coord(100), Coord(80)),
            scale: Scale120(120),
            focus: Focus::Focused,
            visibility: Visibility::Shown,
            trust: WindowTrust::Native,
        },
        frame: frame(byte),
        tree,
        context: ContextObs::Absent,
        cursor: CursorObs::Unknown,
        prev: vec![],
        notes: vec![],
        withheld: cua_run::Withheld::default(),
    }
}

pub fn click_at(x: u32, y: u32) -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: Target::Point(Point::new(Coord(x), Coord(y))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

pub fn click_node(id: u32) -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: Target::Node(cua_action::NodeId(id)),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

pub fn wait(ms: u32) -> CuaAction<WindowSpace> {
    CuaAction::Wait {
        for_ms: WaitMs::new(ms).unwrap(),
    }
}

pub fn finish(outcome: FinishOutcome, text: &str) -> CuaAction<WindowSpace> {
    CuaAction::Finish {
        outcome,
        summary: Summary::new(text).unwrap(),
        extracted: vec![],
    }
}

pub fn reply(actions: Vec<CuaAction<WindowSpace>>) -> Box<ModelReply> {
    Box::new(ModelReply {
        reply: CuaStepReply::new(actions).with_thought("think".to_owned()),
        usage: TokenUsage {
            input: Tokens(0),
            output: Tokens(0),
            cached: Tokens(0),
        },
        cost: MicroUsd(0),
    })
}

/// A run and the effects its last input produced.
#[derive(Clone)]
pub struct Drive {
    pub run: Run,
    pub effects: Vec<RunEffect>,
    pub at: Stamp,
}

impl Drive {
    /// A run started on `kind` for `task`, at instant 0, with its first effects.
    pub fn start(kind: BackendKind, task: CuaTask) -> Drive {
        let (run, effects) = Run::start(run_id(), task, env(kind), Stamp(0));
        Drive {
            run,
            effects,
            at: Stamp(0),
        }
    }

    /// Feeds `input` `after_ms` later than the last one.
    pub fn feed_after(&mut self, after_ms: u64, input: RunInput) -> &mut Drive {
        self.at = Stamp(self.at.0 + after_ms);
        let (run, effects) = self.run.clone().step(input, self.at);
        self.run = run;
        self.effects = effects;
        self
    }

    pub fn feed(&mut self, input: RunInput) -> &mut Drive {
        self.feed_after(10, input)
    }

    /// Brings a started run to `Observing(1)` through the consent and the lease.
    pub fn leased(kind: BackendKind, task: CuaTask) -> Drive {
        let mut d = Drive::start(kind, task);
        d.feed(RunInput::Granted(docket_core::GrantAnswer::Granted));
        d.feed(RunInput::Leased(grant(kind)));
        d
    }

    /// Brings a leased run to `Thinking(1)` having observed `byte`.
    pub fn thinking(kind: BackendKind, task: CuaTask, tree: TreeObs) -> Drive {
        let mut d = Drive::leased(kind, task);
        d.feed(RunInput::Observed(Box::new(observation(1, tree))));
        d
    }

    pub fn phase(&self) -> &RunPhase {
        &self.run.phase
    }

    pub fn has(&self, wanted: impl Fn(&RunEffect) -> bool) -> bool {
        self.effects.iter().any(wanted)
    }
}

pub fn count(n: u32) -> Count {
    Count(n)
}

/// A stock run waiting for the router's verdict on the first of three clicks.
pub fn authorising() -> Drive {
    let mut d = Drive::thinking(
        BackendKind::KwinStock,
        task(SuccessCheck::ModelSays),
        tree(vec![]),
    );
    d.feed(RunInput::ModelReplied(reply(vec![
        click_at(1, 1),
        click_at(2, 2),
        click_at(3, 3),
    ])));
    assert_eq!(*d.phase(), RunPhase::Authorising(StepIndex(1)));
    d
}
