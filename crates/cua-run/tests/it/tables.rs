//! What the pure functions of cua-run must do.

use crate::support::rules;
use a11y_tree::{
    A11yNode, A11ySnapshot, NodeState, NodeValue, Parent, Role, Sensitivity, TreeSource,
    Truncation, TypedHint,
};
use cua_action::{
    Button, Chord, ClickCount, Coord, CuaAction, DeviceSize, FinishOutcome, PixelFormat, Point,
    Rect, Repeat, Scale120, Size, Summary, Target, TypedText, WaitMs, WindowSpace,
};
use cua_action::{WindowClass, WindowTrust};
use cua_bus::WindowRef;
use cua_desktop::{Digest32, Stamp, ToplevelIdent};
use cua_run::{
    ContextObs, CursorObs, Focus, LeaseBook, Observation, Repeats, Stall, TreeObs, Visibility,
    WindowInfo, classify_effect, turn,
};
use docket_core::EffectBasis;
use maskcap::{Frame, FrameDigest, FrameSeq, MaskReport, PixelBuf, WholeMask};
use porter_core::{AppName, Count, DataClass};
use porter_infer::{MaskedRegions, StepIndex, TreeText};
use prov::{
    ActionName, Confidentiality, Effect, Integrity, Label, Labelled, RunId, Source, SpaceId,
};
use std::collections::BTreeSet;
use std::sync::Arc;

fn point() -> Target<WindowSpace> {
    Target::Point(Point::new(Coord(10), Coord(10)))
}

fn click() -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: point(),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

fn typing() -> CuaAction<WindowSpace> {
    CuaAction::Type {
        text: TypedText::new("hello").unwrap(),
    }
}

fn enter() -> CuaAction<WindowSpace> {
    CuaAction::Key {
        chord: Chord::new(BTreeSet::new(), keyboard_types::Key::Enter).unwrap(),
        repeat: Repeat::ONCE,
    }
}

fn chord_press(mods: &[cua_action::Modifier], key: keyboard_types::Key) -> CuaAction<WindowSpace> {
    CuaAction::Key {
        chord: Chord::new(mods.iter().copied().collect(), key).unwrap(),
        repeat: Repeat::ONCE,
    }
}

fn screen_label() -> Label {
    let space = SpaceId::parse("desktop").unwrap();
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space])),
        classes: BTreeSet::from([DataClass::Screen]),
        sources: BTreeSet::from([Source::Screen {
            app: AppName::parse("org.mozilla.firefox").unwrap(),
        }]),
    }
}

fn node_with_typed_action() -> A11yNode {
    let mut node = secret_node(1, Role::Button, "Archive", Sensitivity::Normal, "");
    node.typed = TypedHint::Action(ActionName::parse("mail.thread.archive").unwrap());
    node
}

fn secret_node(id: u32, role: Role, name: &str, sensitivity: Sensitivity, value: &str) -> A11yNode {
    A11yNode {
        id: cua_action::NodeId(id),
        parent: if id == 0 {
            Parent::Root
        } else {
            Parent::Node(cua_action::NodeId(0))
        },
        role,
        name: Labelled::new(name.to_owned(), screen_label()),
        value: if value.is_empty() {
            NodeValue::None
        } else {
            NodeValue::Text(Labelled::new(value.to_owned(), screen_label()))
        },
        states: BTreeSet::from([NodeState::Enabled, NodeState::Visible]),
        bounds: Rect {
            origin: Point::new(Coord(0), Coord(0)),
            size: Size::new(Coord(50), Coord(20)),
        },
        actions: vec![],
        typed: TypedHint::None,
        sensitivity,
    }
}

#[test]
fn effects_that_talk_to_a_neighbour_have_a_deadline_and_the_ones_that_wait_on_the_person_have_none()
{
    use cua_run::{HintSource, RunEffect, RunInput, deadline_of};
    use maskcap::{CaptureError, FrameWant};
    let capture = RunEffect::Capture {
        want: FrameWant::Now,
        hints: HintSource::FromTree,
    };
    let deadline = deadline_of(&capture).expect("a capture is bounded");
    assert!(matches!(
        deadline.failure,
        Some(RunInput::CaptureFailed(CaptureError::Protocol(_)))
    ));
    let consent = RunEffect::Grant {
        app: AppName::parse("org.gnome.Calculator").expect("an app"),
        space: SpaceId::parse("desktop").expect("a space"),
        level: docket_core::Level::AlwaysAsk,
    };
    // The consent waits on a person, so its wait is long, but it ends: a run never stays in
    // `Granting` for ever.
    let wait = deadline_of(&consent).expect("the consent wait is bounded");
    assert!(wait.after >= std::time::Duration::from_secs(120));
    assert_eq!(wait.failure, Some(RunInput::GrantUnanswered));
    assert_eq!(deadline_of(&RunEffect::Proceed), None);
}

#[test]
fn classify_effect_table() {
    use WindowClass::{MailCompose, Ordinary, Payments, Terminal};
    struct Row {
        name: &'static str,
        action: CuaAction<WindowSpace>,
        class: WindowClass,
        want: (Effect, EffectBasis),
    }
    let observe = CuaAction::<WindowSpace>::Observe;
    let wait = CuaAction::<WindowSpace>::Wait {
        for_ms: WaitMs::new(500).unwrap(),
    };
    let finish = CuaAction::<WindowSpace>::Finish {
        outcome: FinishOutcome::Done,
        summary: Summary::new("done").unwrap(),
        extracted: vec![],
    };
    let rows = [
        Row {
            name: "observe",
            action: observe,
            class: Ordinary,
            want: (Effect::Read, EffectBasis::DefaultTable),
        },
        Row {
            name: "wait",
            action: wait,
            class: Ordinary,
            want: (Effect::Read, EffectBasis::DefaultTable),
        },
        Row {
            name: "finish",
            action: finish,
            class: Ordinary,
            want: (Effect::Read, EffectBasis::DefaultTable),
        },
        Row {
            name: "click",
            action: click(),
            class: Ordinary,
            want: (Effect::UndoableWrite, EffectBasis::DefaultTable),
        },
        Row {
            name: "type",
            action: typing(),
            class: Ordinary,
            want: (Effect::UndoableWrite, EffectBasis::DefaultTable),
        },
        Row {
            name: "type in compose",
            action: typing(),
            class: MailCompose,
            want: (Effect::Outbound, EffectBasis::WindowClass(MailCompose)),
        },
        Row {
            name: "enter in compose",
            action: enter(),
            class: MailCompose,
            want: (Effect::Outbound, EffectBasis::WindowClass(MailCompose)),
        },
        Row {
            name: "type in a terminal",
            action: typing(),
            class: Terminal,
            want: (Effect::Outbound, EffectBasis::WindowClass(Terminal)),
        },
        Row {
            name: "enter in payments",
            action: enter(),
            class: Payments,
            want: (Effect::Outbound, EffectBasis::WindowClass(Payments)),
        },
        Row {
            name: "type in a bank",
            action: typing(),
            class: WindowClass::Banking,
            want: (
                Effect::Outbound,
                EffectBasis::WindowClass(WindowClass::Banking),
            ),
        },
        Row {
            name: "click in a terminal is not typing",
            action: click(),
            class: Terminal,
            want: (Effect::UndoableWrite, EffectBasis::WindowClass(Terminal)),
        },
        Row {
            name: "a click in a password manager names the class",
            action: click(),
            class: WindowClass::PasswordManager,
            want: (
                Effect::UndoableWrite,
                EffectBasis::WindowClass(WindowClass::PasswordManager),
            ),
        },
        Row {
            name: "Command+C stays an ordinary write",
            action: chord_press(
                &[cua_action::Modifier::Super],
                keyboard_types::Key::Character("c".into()),
            ),
            class: Ordinary,
            want: (Effect::UndoableWrite, EffectBasis::DefaultTable),
        },
        Row {
            name: "Command+Tab leaves the window, so the router asks",
            action: chord_press(&[cua_action::Modifier::Super], keyboard_types::Key::Tab),
            class: Ordinary,
            want: (Effect::Outbound, EffectBasis::DefaultTable),
        },
        Row {
            name: "the desktop's chord is outbound in any window",
            action: chord_press(&[cua_action::Modifier::Super], keyboard_types::Key::Tab),
            class: Terminal,
            want: (Effect::Outbound, EffectBasis::DefaultTable),
        },
        Row {
            name: "a key in an admin tool names the class",
            action: enter(),
            class: WindowClass::Admin,
            want: (
                Effect::UndoableWrite,
                EffectBasis::WindowClass(WindowClass::Admin),
            ),
        },
    ];
    for row in rows {
        assert_eq!(
            classify_effect(&rules().commit, &row.action, None, row.class),
            row.want,
            "{}",
            row.name
        );
    }
    let node = node_with_typed_action();
    let (_, basis) = classify_effect(&rules().commit, &click(), Some(&node), MailCompose);
    assert_eq!(
        basis,
        EffectBasis::NodeTypedAction(ActionName::parse("mail.thread.archive").unwrap()),
        "a typed node wins over the window class"
    );
}

#[test]
fn a_click_on_a_commit_control_in_a_form_window_is_outbound() {
    use WindowClass::{Banking, MailCompose, Ordinary, Payments, Terminal};
    let named = |role, name: &str| secret_node(1, role, name, Sensitivity::Normal, "");
    let outbound = |class| (Effect::Outbound, EffectBasis::WindowClass(class));
    let plain = |class| match class {
        Ordinary => (Effect::UndoableWrite, EffectBasis::DefaultTable),
        _ => (Effect::UndoableWrite, EffectBasis::WindowClass(class)),
    };
    let rows = [
        ("Send", Role::Button, MailCompose, true),
        ("Pay now", Role::Button, Payments, true),
        ("Confirm transfer", Role::Button, Banking, true),
        ("Submit order", Role::Link, Payments, true),
        ("Send & close", Role::MenuItem, MailCompose, true),
        ("Back", Role::Button, Payments, false),
        ("Discard", Role::Button, MailCompose, false),
        ("Sender", Role::Button, MailCompose, false),
        ("Send", Role::Entry, MailCompose, false),
        ("Send", Role::Button, Ordinary, false),
        ("Send", Role::Button, Terminal, false),
    ];
    for (name, role, class, commits) in rows {
        let want = if commits {
            outbound(class)
        } else {
            plain(class)
        };
        let node = named(role, name);
        assert_eq!(
            classify_effect(&rules().commit, &click(), Some(&node), class),
            want,
            "{name} {role:?} {class:?}"
        );
    }
    assert_eq!(
        classify_effect(&rules().commit, &click(), None, Payments),
        outbound(Payments),
        "a click on an unknown node in a payment form might be the commit"
    );
    assert_eq!(
        classify_effect(&rules().commit, &click(), None, Ordinary),
        plain(Ordinary)
    );
}

#[test]
fn a_pay_button_on_screen_makes_an_untitled_window_a_payment_form() {
    let app = AppName::parse("org.mozilla.firefox").unwrap();
    let pay = secret_node(1, Role::Button, "Pay now", Sensitivity::Normal, "");
    let back = secret_node(2, Role::Button, "Back", Sensitivity::Normal, "");
    let class = |nodes: &[A11yNode]| cua_run::window_class_in(&rules(), &app, "Shop", nodes);
    assert_eq!(class(&[back.clone(), pay]), WindowClass::Payments);
    assert_eq!(class(&[back]), WindowClass::Ordinary);
    assert_eq!(class(&[]), WindowClass::Ordinary);
}

fn digest(byte: u8) -> FrameDigest {
    FrameDigest(Digest32([byte; 32]))
}

#[test]
fn stall_after_n_equal_digests() {
    struct Row {
        name: &'static str,
        history: Vec<FrameDigest>,
        limit: u32,
        want: Stall,
    }
    let (a, b) = (digest(1), digest(2));
    let rows = [
        Row {
            name: "no frames",
            history: vec![],
            limit: 3,
            want: Stall::Moving,
        },
        Row {
            name: "fewer than the limit",
            history: vec![a, a],
            limit: 3,
            want: Stall::Moving,
        },
        Row {
            name: "the newest three equal",
            history: vec![a, a, a],
            limit: 3,
            want: Stall::Stalled,
        },
        Row {
            name: "older ones do not matter",
            history: vec![a, b, b, b],
            limit: 3,
            want: Stall::Stalled,
        },
        Row {
            name: "the last one moved",
            history: vec![a, a, b],
            limit: 3,
            want: Stall::Moving,
        },
        Row {
            name: "a limit of zero never stalls",
            history: vec![a, a, a],
            limit: 0,
            want: Stall::Moving,
        },
    ];
    for row in rows {
        assert_eq!(
            row.history
                .iter()
                .fold(Repeats::default(), |seen, d| seen.seen(*d))
                .stall(Count(row.limit)),
            row.want,
            "{}",
            row.name
        );
    }
}

#[test]
fn lease_book_one_run_per_window() {
    let (w1, w2) = (
        ToplevelIdent::parse("w1").unwrap(),
        ToplevelIdent::parse("w2").unwrap(),
    );
    let (r1, r2) = (RunId::parse("r-1").unwrap(), RunId::parse("r-2").unwrap());
    let book = LeaseBook::default().claim(&w1, &r1).expect("free window");
    assert_eq!(book.holder(&w1), Some(&r1));
    assert_eq!(
        book.claim(&w1, &r2).unwrap_err().by,
        r1,
        "a held window is refused"
    );
    assert_eq!(
        book.claim(&w2, &r1).unwrap_err().by,
        r1,
        "one window per run"
    );
    let book = book.set_state(&r1, cua_run::SlotState::Suspended);
    assert_eq!(book.holder(&w1), Some(&r1), "suspended is still held");
    assert_eq!(book.claim(&w1, &r2).unwrap_err().by, r1);
    let book = book.release(&r1);
    assert_eq!(book.holder(&w1), None);
    assert!(book.claim(&w1, &r2).is_ok());
}

fn observation(tree: A11ySnapshot, masked: usize) -> Observation {
    let regions = (0..masked)
        .map(|i| maskcap::MaskRegion {
            rect: Rect {
                origin: Point::new(Coord(i as u32), Coord(0)),
                size: Size::new(Coord(5), Coord(5)),
            },
            why: maskcap::MaskReason::PasswordField,
        })
        .collect();
    Observation {
        step: StepIndex(1),
        window: WindowInfo {
            window: WindowRef(1),
            app: AppName::parse("org.mozilla.firefox").unwrap(),
            title: Labelled::new("Sign in".to_owned(), screen_label()),
            size: Size::new(Coord(100), Coord(80)),
            scale: Scale120(120),
            focus: Focus::Focused,
            visibility: Visibility::Shown,
            trust: WindowTrust::Native,
        },
        frame: Frame {
            seq: FrameSeq(1),
            at: Stamp(1000),
            pixels: PixelBuf {
                format: PixelFormat::Xbgr8888,
                size: DeviceSize { w: 100, h: 80 },
                stride: 400,
                data: Arc::from(vec![0u8; 400 * 80]),
            },
            logical: Size::new(Coord(100), Coord(80)),
            scale: Scale120(120),
            damage: vec![],
            masks: MaskReport {
                applied_by: cua_desktop::MaskWhere::Cuad,
                whole: WholeMask::None,
                regions,
            },
        },
        tree: TreeObs::Present(tree),
        context: ContextObs::Absent,
        cursor: CursorObs::Unknown,
        prev: vec![],
        notes: vec![],
        withheld: cua_run::Withheld::default(),
    }
}

fn form_tree() -> A11ySnapshot {
    A11ySnapshot {
        source: TreeSource::Atspi,
        taken: Stamp(900),
        nodes: vec![
            secret_node(0, Role::Document, "Sign in", Sensitivity::Normal, ""),
            secret_node(
                1,
                Role::Entry,
                "Email",
                Sensitivity::Normal,
                "ada@example.org",
            ),
            secret_node(
                2,
                Role::PasswordText,
                "Password",
                Sensitivity::Secret,
                "hunter2-hunter2",
            ),
        ],
        truncated: Truncation::Complete,
    }
}

#[test]
fn turn_renders_tree_without_secret_values_and_notes_masked_regions() {
    let (request, attachment) = turn(&observation(form_tree(), 2));
    let TreeText::Present(text) = &request.tree else {
        panic!("a tree was observed")
    };
    assert!(text.contains("ada@example.org"), "{text}");
    assert!(
        !text.contains("hunter2"),
        "a secret value never reaches the model: {text}"
    );
    assert_eq!(request.masked, MaskedRegions(2));
    assert_eq!(request.step, StepIndex(1));
    assert_eq!(attachment.0.size, DeviceSize { w: 100, h: 80 });
    let none = observation(form_tree(), 0);
    let none = Observation {
        tree: TreeObs::Absent(cua_run::TreeAbsence::NotExposed),
        ..none
    };
    assert_eq!(turn(&none).0.tree, TreeText::Absent);
}

#[test]
fn repeats_count_the_newest_run_of_equal_digests_in_constant_space() {
    let (a, b) = (digest(1), digest(2));
    let counts: Vec<u32> = [a, a, b, b, b]
        .iter()
        .scan(Repeats::default(), |seen, d| {
            *seen = seen.seen(*d);
            Some(seen.count().0)
        })
        .collect();
    assert_eq!(counts, [1, 2, 1, 2, 3]);
    let long = (0..100_000).fold(Repeats::default(), |seen, _| seen.seen(a));
    assert_eq!(long.count(), Count(100_000));
}
