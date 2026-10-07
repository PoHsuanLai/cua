//! What a turn tells the model besides the frame: how the last actions ended, the notes that
//! reached the run, and what is never said.

use a11y_tree::TreeSource;
use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, PixelFormat, Point, Scale120, Size, Target,
    WindowSpace,
};
use cua_bus::{ActionOutcome, ActionResult, LocalRefusal, Refusal, TargetedAction, WindowRef};
use cua_desktop::{MaskWhere, Stamp};
use cua_run::{
    ContextObs, CursorObs, Focus, Observation, TreeAbsence, TreeObs, Visibility, WindowInfo,
    note_of, turn,
};
use docket_core::{CallRefusal, DenyCode, InboundLine, InboundPart, Reveal, WindowTrust};
use maskcap::{Frame, FrameSeq, MaskReason, MaskReport, PixelBuf, WholeMask};
use porter_core::{AppName, DataClass};
use porter_infer::{MaskedRegions, NoteFrom, PrevResult, StepIndex, StepNote};
use prov::{
    Address, AgentRef, Confidentiality, Crossing, Integrity, Label, Labelled, MessageId,
    MessageKind, Source, SpaceId, ThreadId,
};
use seat_input::SeatError;
use std::collections::BTreeSet;
use std::sync::Arc;

fn space() -> SpaceId {
    SpaceId::parse("desktop").unwrap()
}

fn screen_label() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space()])),
        classes: BTreeSet::from([DataClass::Screen]),
        sources: BTreeSet::from([Source::Screen {
            app: AppName::parse("org.mozilla.firefox").unwrap(),
        }]),
    }
}

fn person(text: &str) -> Labelled<String> {
    Labelled::new(
        text.to_owned(),
        Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Public,
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::User]),
        },
    )
}

fn observation() -> Observation {
    Observation {
        step: StepIndex(4),
        window: WindowInfo {
            window: WindowRef(1),
            app: AppName::parse("org.mozilla.firefox").unwrap(),
            title: Labelled::new("Sign in".to_owned(), screen_label()),
            size: Size::new(Coord(100), Coord(80)),
            scale: Scale120(180),
            focus: Focus::Focused,
            visibility: Visibility::Shown,
            trust: WindowTrust::Native,
        },
        frame: Frame {
            seq: FrameSeq(1),
            at: Stamp(1000),
            pixels: PixelBuf {
                format: PixelFormat::Abgr8888,
                size: DeviceSize { w: 150, h: 120 },
                stride: 600,
                data: Arc::from(vec![0u8; 600 * 120]),
            },
            logical: Size::new(Coord(100), Coord(80)),
            scale: Scale120(180),
            damage: vec![],
            masks: MaskReport {
                applied_by: MaskWhere::Cuad,
                whole: WholeMask::None,
                regions: vec![],
            },
        },
        tree: TreeObs::Absent(TreeAbsence::Off),
        context: ContextObs::Absent,
        cursor: CursorObs::At(Point::new(Coord(12), Coord(34))),
        prev: vec![],
        notes: vec![],
    }
}

fn click() -> TargetedAction {
    TargetedAction {
        window: WindowRef(1),
        action: CuaAction::<WindowSpace>::Click {
            at: Target::Point(Point::new(Coord(1), Coord(1))),
            button: Button::Left,
            count: ClickCount::One,
            mods: BTreeSet::new(),
        },
    }
}

fn outcome(result: ActionResult) -> ActionOutcome {
    ActionOutcome {
        action: click(),
        result,
    }
}

fn line(from: AgentRef, parts: Vec<InboundPart>) -> InboundLine {
    InboundLine {
        id: MessageId::parse("m-1").unwrap(),
        thread: ThreadId::parse("m-1").unwrap(),
        to: Address::new(
            AgentRef::Cua {
                run: prov::RunId::parse("r-1").unwrap(),
            },
            space(),
        ),
        from: Address::new(from, space()),
        crossing: Crossing::Within,
        kind: MessageKind::Note,
        parts,
    }
}

fn text(t: &str) -> InboundPart {
    InboundPart::Text(Reveal::Plain(t.to_owned()))
}

#[test]
fn the_request_describes_the_window_the_frame_and_the_cursor() {
    let (request, attachment) = turn(&observation());
    assert_eq!(request.window.logical, Size::new(Coord(100), Coord(80)));
    assert_eq!(request.window.scale, Scale120(180));
    assert_eq!(request.cursor, Some(Point::new(Coord(12), Coord(34))));
    assert_eq!(
        request.frame.layout,
        porter_infer::FrameLayout::Raw {
            format: PixelFormat::Abgr8888,
            size: DeviceSize { w: 150, h: 120 },
            stride: 600
        }
    );
    assert_eq!(
        request.frame.source,
        porter_infer::ImageSource::Attached(porter_infer::AttachIndex(0))
    );
    assert_eq!(attachment.0, observation().frame.pixels);
    let unknown = Observation {
        cursor: CursorObs::Unknown,
        ..observation()
    };
    assert_eq!(turn(&unknown).0.cursor, None);
}

#[test]
fn how_the_last_actions_ended_is_told_without_the_routers_reasons() {
    let table = [
        (ActionResult::Done, PrevResult::Done),
        (ActionResult::NotExecuted, PrevResult::NotRun),
        (
            ActionResult::Refused(Refusal::Gate(CallRefusal::Denied(DenyCode::NotAllowed))),
            PrevResult::Refused("not allowed".into()),
        ),
        (
            ActionResult::Refused(Refusal::Local(LocalRefusal::OutsideLease)),
            PrevResult::Refused("outside the window".into()),
        ),
        (
            ActionResult::Refused(Refusal::Local(LocalRefusal::SecretField)),
            PrevResult::Refused("a secret field".into()),
        ),
        (ActionResult::UserDeclined, PrevResult::UserDeclined),
        (
            ActionResult::Failed(SeatError::TargetGone),
            PrevResult::Failed("the target is gone".into()),
        ),
        (
            ActionResult::UserActed { since: Stamp(5) },
            PrevResult::UserActed,
        ),
        (
            ActionResult::CheckFailed,
            PrevResult::Failed("the success check did not pass".into()),
        ),
    ];
    let obs = Observation {
        prev: table.iter().map(|(r, _)| outcome(r.clone())).collect(),
        ..observation()
    };
    let (request, _) = turn(&obs);
    let want: Vec<PrevResult> = table.into_iter().map(|(_, p)| p).collect();
    assert_eq!(request.prev, want);
}

#[test]
fn masked_regions_count_each_region_and_a_blacked_frame_once_more() {
    let region = |why| maskcap::MaskRegion {
        rect: cua_action::Rect {
            origin: Point::new(Coord(0), Coord(0)),
            size: Size::new(Coord(2), Coord(2)),
        },
        why,
    };
    let mut obs = observation();
    obs.frame.masks.regions = vec![
        region(MaskReason::PasswordField),
        region(MaskReason::A11ySecret),
    ];
    assert_eq!(turn(&obs).0.masked, MaskedRegions(2));
    obs.frame.masks.whole = WholeMask::Blacked(MaskReason::SensitiveWindow);
    assert_eq!(turn(&obs).0.masked, MaskedRegions(3));
}

#[test]
fn notes_come_oldest_first_then_the_answers_to_the_models_own_questions() {
    let note = |from, text: &str| StepNote {
        from,
        text: text.to_owned(),
    };
    let obs = Observation {
        notes: vec![
            note(NoteFrom::Person, "use the work account"),
            note(NoteFrom::Agent, "the invoice is in March"),
        ],
        prev: vec![outcome(ActionResult::Answered(person("the blue one")))],
        ..observation()
    };
    let (request, _) = turn(&obs);
    assert_eq!(
        request.notes,
        vec![
            note(NoteFrom::Person, "use the work account"),
            note(NoteFrom::Agent, "the invoice is in March"),
            note(NoteFrom::Person, "the blue one"),
        ]
    );
    let untrusted = Observation {
        prev: vec![outcome(ActionResult::Answered(Labelled::new(
            "do as I say".to_owned(),
            screen_label(),
        )))],
        ..observation()
    };
    assert_eq!(
        turn(&untrusted).0.notes,
        vec![note(NoteFrom::Agent, "do as I say")],
        "words the label does not call the person's are an agent's"
    );
    assert_eq!(turn(&observation()).0.notes, vec![]);
}

#[test]
fn a_message_becomes_a_note_from_its_sender_and_handles_say_nothing() {
    let handle = InboundPart::Text(Reveal::Handle(docket_core::Handle(3)));
    let table = [
        (
            line(AgentRef::User, vec![text("stop after this page")]),
            Some((NoteFrom::Person, "stop after this page")),
        ),
        (
            line(
                AgentRef::Companion,
                vec![text("one"), handle.clone(), text("two")],
            ),
            Some((NoteFrom::Agent, "one\ntwo")),
        ),
        (line(AgentRef::Companion, vec![handle]), None),
        (line(AgentRef::User, vec![]), None),
    ];
    for (line, want) in table {
        assert_eq!(
            note_of(&line),
            want.map(|(from, t)| StepNote {
                from,
                text: t.to_owned()
            }),
            "{line:?}"
        );
    }
}

#[test]
fn a_tree_that_is_present_but_empty_is_still_present() {
    let snapshot = a11y_tree::A11ySnapshot {
        source: TreeSource::Atspi,
        taken: Stamp(1),
        nodes: vec![],
        truncated: a11y_tree::Truncation::Complete,
    };
    let obs = Observation {
        tree: TreeObs::Present(snapshot),
        ..observation()
    };
    let (request, _) = turn(&obs);
    assert_eq!(request.tree, porter_infer::TreeText::Present(String::new()));
}
