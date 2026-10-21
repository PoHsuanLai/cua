//! The fakes do what their docs say: they are the doubles every other crate's tests lean on, so
//! they are tested like code.

use cua_action::{Coord, DeviceSize, PixelFormat, Point, Rect, Scale120, Size};
use cua_bus::CuaRecord;
use cua_desktop::{
    BackendKind, Desktop, DesktopError, DesktopEvent, LeaseControl, LeaseRequest, MaskWhere,
    ReleasePlacement, RunMode, Stamp, ToplevelIdent, ToplevelInfo, WindowTrust,
};
use cua_fake::{
    DesktopCall, FakeCapture, FakeDesktop, FakePdp, MemoryEvents, ScriptedModel, Verdict,
};
use cua_run::{
    CuaModel, CuaModelError, FrameAttachment, GateCheck, GateEvent, GateLink, MessageLink,
    RecordSink, StepSink,
};
use docket_core::{
    CallRefusal, ConfirmEnd, CuaAsk, DraftPart, EffectBasis, GateAnswer, GrantAnswer, GrantAsk,
    InboxAsk, MessageDraft,
};
use maskcap::{
    CaptureError, CaptureSession, Frame, FrameSeq, MaskReport, PixelBuf, WholeMask, WindowCapture,
};
use porter_core::{AppName, DataClass, Px};
use porter_infer::{CuaBegin, CuaStepReply, StepIndex};
use prov::{
    Address, AgentRef, Confidentiality, Effect, Integrity, Label, MessageKind, MessageText, RunId,
    SessionId, Source, SpaceId,
};
use std::collections::BTreeSet;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// Polls a future that never waits to its result.
fn ready<T>(future: impl Future<Output = T>) -> T {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("the fake never waits"),
    }
}

fn space() -> SpaceId {
    SpaceId::parse("desktop").unwrap()
}

fn app() -> AppName {
    AppName::parse("org.mozilla.firefox").unwrap()
}

fn window(ident: &str, trust: WindowTrust) -> ToplevelInfo {
    ToplevelInfo {
        ident: ToplevelIdent::parse(ident).unwrap(),
        app: app(),
        title: cua_desktop::WindowTitle("Invoices".into()),
        pid: cua_desktop::Pid(77),
        geometry: cua_desktop::GlobalRect {
            x: cua_desktop::GlobalCoord(0),
            y: cua_desktop::GlobalCoord(0),
            w: Px(800),
            h: Px(600),
        },
        workspace: cua_desktop::WorkspaceRef {
            id: "ws-1".into(),
            shown: cua_desktop::WorkspaceShown::Shown,
        },
        state: BTreeSet::new(),
        trust,
        parent: None,
    }
}

fn request(ident: &str, mode: RunMode) -> LeaseRequest {
    LeaseRequest {
        window: ToplevelIdent::parse(ident).unwrap(),
        mode,
        space: space(),
        placement: ReleasePlacement::ReturnToUser,
    }
}

fn ask() -> CuaAsk {
    let label = Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space()])),
        classes: BTreeSet::from([DataClass::Screen]),
        sources: BTreeSet::from([Source::Screen { app: app() }]),
    };
    CuaAsk {
        run: RunId::parse("r-1").unwrap(),
        step: 1,
        app: app(),
        trust: WindowTrust::Native,
        mode: RunMode::InPlace,
        space: space(),
        action: cua_action::CuaAction::Observe,
        node: None,
        effect: Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: label,
    }
}

#[test]
fn the_fake_gate_answers_from_its_table_and_remembers_what_it_was_asked() {
    let pdp = FakePdp::scripted(
        vec![GrantAnswer::Refused(ConfirmEnd::Refused)],
        vec![
            Verdict::Run,
            Verdict::Refuse(CallRefusal::Timeout),
            Verdict::AskThen(GateAnswer::Run),
        ],
    );
    let grant = GrantAsk {
        app: app(),
        space: space(),
        level: docket_core::Level::AlwaysAllow,
        run: None,
    };
    assert_eq!(
        ready(pdp.grant(grant.clone())),
        Ok(GrantAnswer::Refused(ConfirmEnd::Refused))
    );
    assert_eq!(
        ready(pdp.grant(grant.clone())),
        Ok(GrantAnswer::Granted),
        "past the table: granted"
    );
    assert_eq!(pdp.grant_asks(), vec![grant.clone(), grant]);

    let mut run = ready(pdp.check(ask())).unwrap();
    assert_eq!(ready(run.next()), Ok(GateEvent::Answered(GateAnswer::Run)));
    let mut refused = ready(pdp.check(ask())).unwrap();
    assert_eq!(
        ready(refused.next()),
        Ok(GateEvent::Answered(GateAnswer::Refused(
            CallRefusal::Timeout
        )))
    );
    let mut asked = ready(pdp.check(ask())).unwrap();
    let Ok(GateEvent::Confirming(id)) = ready(asked.next()) else {
        panic!("asks first")
    };
    assert_eq!(id.as_str(), "c-1");
    ready(asked.proceed()).unwrap();
    assert_eq!(asked.proceeded(), 1);
    assert_eq!(
        ready(asked.next()),
        Ok(GateEvent::Answered(GateAnswer::Run))
    );
    assert_eq!(pdp.checks().len(), 3);
    let mut past = ready(pdp.check(ask())).unwrap();
    assert_eq!(
        ready(past.next()),
        Ok(GateEvent::Answered(GateAnswer::Run)),
        "past the table: run"
    );
}

fn report_draft() -> MessageDraft {
    MessageDraft {
        to: Address::new(AgentRef::Companion, space()),
        thread: None,
        in_reply_to: None,
        kind: MessageKind::Report {
            status: prov::ReportStatus::Done,
        },
        parts: vec![DraftPart::Text(MessageText::new("Done."))],
    }
}

#[test]
fn memory_events_keep_records_and_reports_and_number_the_messages() {
    let events = MemoryEvents::new();
    let session = SessionId::parse("s-1").unwrap();
    let first = ready(events.send(session.clone(), report_draft())).unwrap();
    let second = ready(events.send(session.clone(), report_draft())).unwrap();
    assert_eq!(first.message.as_str(), "m-1");
    assert_eq!(second.message.as_str(), "m-2");
    assert_eq!(
        first.thread.as_str(),
        "m-1",
        "a report that names no thread starts one"
    );
    assert_eq!(events.sent().len(), 2);
    assert_eq!(events.sent()[0].0, session);
    let record = CuaRecord::TakenOver {
        run: RunId::parse("r-1").unwrap(),
        n: StepIndex(2),
    };
    ready(events.record(record.clone())).unwrap();
    assert_eq!(events.records(), vec![record]);
    let ask = InboxAsk {
        agent: AgentRef::User,
        after: None,
    };
    assert_eq!(ready(events.inbox(ask)).unwrap(), vec![]);
}

#[test]
fn the_fake_desktop_leases_what_the_backend_supports_and_refuses_the_rest() {
    let windows = vec![
        window("w1", WindowTrust::Native),
        window("shell", WindowTrust::Shell),
    ];
    let stock = FakeDesktop::new(BackendKind::KwinStock, windows.clone());
    let first = ready(stock.lease(request("w1", RunMode::InPlace))).expect("in place works");
    assert_eq!(first.grant.seat, cua_desktop::SeatIsolation::SharedSeat);
    assert_eq!(first.grant.ei, cua_desktop::EiOrigin::SenderSocket);
    assert_eq!(
        ready(stock.lease(request("w1", RunMode::AgentWorkspace))).unwrap_err(),
        DesktopError::ModeUnsupported(RunMode::AgentWorkspace),
        "stock renders hidden windows but delivers them no input"
    );
    assert_eq!(
        ready(stock.lease(request("shell", RunMode::InPlace))).unwrap_err(),
        DesktopError::NotLeasable(WindowTrust::Shell)
    );
    assert_eq!(
        ready(stock.lease(request("nope", RunMode::InPlace))).unwrap_err(),
        DesktopError::NoSuchWindow
    );
    let leased = ready(stock.lease(request("w1", RunMode::InPlace))).unwrap();
    ready(stock.suspend(leased.grant.id, cua_desktop::Suspend::Consent)).unwrap();
    ready(stock.release(leased.grant.id)).unwrap();
    assert_eq!(
        stock.calls(),
        vec![
            DesktopCall::Leased(first.grant.id),
            DesktopCall::Leased(leased.grant.id),
            DesktopCall::Suspended(leased.grant.id, cua_desktop::Suspend::Consent),
            DesktopCall::Released(leased.grant.id),
        ]
    );
    stock.raise(DesktopEvent::KillChord);
    assert_eq!(ready(stock.next_event()), DesktopEvent::KillChord);
}

fn frame(seq: u64) -> Frame {
    Frame {
        seq: FrameSeq(seq),
        at: Stamp(seq * 100),
        pixels: PixelBuf {
            format: PixelFormat::Xbgr8888,
            size: DeviceSize { w: 1, h: 1 },
            stride: 4,
            data: Arc::from(vec![0u8; 4]),
        },
        logical: Size::new(Coord(1), Coord(1)),
        scale: Scale120(120),
        damage: vec![Rect {
            origin: Point::new(Coord(0), Coord(0)),
            size: Size::new(Coord(1), Coord(1)),
        }],
        masks: MaskReport {
            applied_by: MaskWhere::Cuad,
            whole: WholeMask::None,
            regions: vec![],
        },
    }
}

#[test]
fn scripted_capture_returns_its_frames_in_order_then_the_window_is_gone() {
    let desktop = FakeDesktop::new(
        BackendKind::KwinStock,
        vec![window("w1", WindowTrust::Native)],
    );
    let grant = ready(desktop.lease(request("w1", RunMode::InPlace)))
        .unwrap()
        .grant;
    let capture = FakeCapture::scripted(vec![frame(1), frame(2)], vec![]);
    let mut session = ready(capture.open(&grant)).unwrap();
    let want = maskcap::FrameWant::Now;
    assert_eq!(ready(session.frame(want, &[])).unwrap().seq, FrameSeq(1));
    assert_eq!(ready(session.frame(want, &[])).unwrap().seq, FrameSeq(2));
    assert_eq!(
        ready(session.frame(want, &[])).unwrap_err(),
        CaptureError::SourceGone
    );
    assert_eq!(
        session.hints_seen().len(),
        3,
        "each call's hints are recorded"
    );
    capture.refuse_next(CaptureError::Refused);
    assert_eq!(
        ready(capture.open(&grant)).unwrap_err(),
        CaptureError::Refused
    );
    assert!(
        ready(capture.open(&grant)).is_ok(),
        "the refusal happens once"
    );
}

struct Sink;
impl StepSink for Sink {
    fn event(&mut self, _event: cua_run::StepEvent) {}
}

fn blank_request() -> porter_infer::CuaStepRequest {
    porter_infer::CuaStepRequest::new(
        StepIndex(1),
        porter_infer::WindowGeometry {
            logical: Size::new(Coord(1), Coord(1)),
            scale: Scale120(120),
        },
        porter_infer::FrameImage {
            source: porter_infer::ImageSource::Attached(porter_infer::AttachIndex(0)),
            layout: porter_infer::FrameLayout::Raw {
                format: PixelFormat::Xbgr8888,
                size: DeviceSize { w: 1, h: 1 },
                stride: 4,
            },
        },
        porter_infer::TreeText::Absent,
    )
}

#[test]
fn the_scripted_model_plays_its_script_then_fails_fatally_and_remembers_the_goal() {
    let reply = CuaStepReply::new(vec![]);
    let mut model = ScriptedModel::new(vec![Ok(reply.clone()), Err(CuaModelError::Transient)]);
    let begin = CuaBegin::new(
        "download the invoice".into(),
        porter_core::capability::CuaEnv::Desktop,
    );
    ready(model.begin(begin.clone())).unwrap();
    assert_eq!(model.begun(), &[begin]);
    let attachment = || FrameAttachment(frame(1).pixels);
    assert_eq!(
        ready(model.step(blank_request(), attachment(), &mut Sink)),
        Ok(reply)
    );
    assert_eq!(
        ready(model.step(blank_request(), attachment(), &mut Sink)),
        Err(CuaModelError::Transient)
    );
    assert_eq!(
        ready(model.step(blank_request(), attachment(), &mut Sink)),
        Err(CuaModelError::Fatal),
        "past the script"
    );
    assert_eq!(model.requests().len(), 3);
}
