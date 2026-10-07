#![allow(dead_code)] // each test binary uses a subset of the shared builders
//! Hand-built snapshots that the fixture files must equal. Each is a small, honest tree of the
//! kind of window the specs name; the JSON under `fixtures/a11y/` is their serde form.

use a11y_tree::{
    A11yNode, A11ySnapshot, NodeAction, NodeState, NodeValue, Parent, Role, Sensitivity,
    TreeSource, Truncation, TypedHint,
};
use cua_action::{Coord, NodeId, Point, Rect, Size};
use cua_desktop::Stamp;
use porter_core::{AppName, DataClass};
use prov::{ActionName, Confidentiality, Integrity, Label, Labelled, Source, SpaceId};
use std::collections::BTreeSet;

pub fn screen(app: &str, text: &str) -> Labelled<String> {
    let label = Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([
            SpaceId::parse("desktop").unwrap()
        ])),
        classes: BTreeSet::from([DataClass::Screen]),
        sources: BTreeSet::from([Source::Screen {
            app: AppName::parse(app).unwrap(),
        }]),
    };
    Labelled::new(text.to_owned(), label)
}

pub struct Spec {
    pub id: u32,
    pub parent: Parent,
    pub role: Role,
    pub name: &'static str,
    pub value: NodeValue,
    pub states: &'static [NodeState],
    pub bounds: (u32, u32, u32, u32),
    pub actions: Vec<NodeAction>,
    pub typed: TypedHint,
    pub sensitivity: Sensitivity,
}

impl Spec {
    pub fn new(
        id: u32,
        parent: Parent,
        role: Role,
        name: &'static str,
        bounds: (u32, u32, u32, u32),
    ) -> Self {
        Spec {
            id,
            parent,
            role,
            name,
            value: NodeValue::None,
            states: &[NodeState::Enabled, NodeState::Visible, NodeState::Showing],
            bounds,
            actions: Vec::new(),
            typed: TypedHint::None,
            sensitivity: Sensitivity::Normal,
        }
    }
}

pub fn snapshot(app: &str, source: TreeSource, specs: Vec<Spec>) -> A11ySnapshot {
    let nodes = specs
        .into_iter()
        .map(|s| {
            let (x, y, w, h) = s.bounds;
            A11yNode {
                id: NodeId(s.id),
                parent: s.parent,
                role: s.role,
                name: screen(app, s.name),
                value: s.value,
                states: s.states.iter().copied().collect(),
                bounds: Rect {
                    origin: Point::new(Coord(x), Coord(y)),
                    size: Size::new(Coord(w), Coord(h)),
                },
                actions: s.actions,
                typed: s.typed,
                sensitivity: s.sensitivity,
            }
        })
        .collect();
    A11ySnapshot {
        source,
        taken: Stamp(1000),
        nodes,
        truncated: Truncation::Complete,
    }
}

const ROOT: Parent = Parent::Root;
fn under(id: u32) -> Parent {
    Parent::Node(NodeId(id))
}

const FOCUSABLE: &[NodeState] = &[
    NodeState::Enabled,
    NodeState::Focusable,
    NodeState::Visible,
    NodeState::Showing,
];

pub fn gtk4_dialog() -> A11ySnapshot {
    let button = |id, name, x| {
        let mut s = Spec::new(id, under(0), Role::Button, name, (x, 120, 80, 32));
        s.states = FOCUSABLE;
        s.actions = vec![NodeAction::Click];
        s
    };
    snapshot(
        "org.gnome.TextEditor",
        TreeSource::Atspi,
        vec![
            Spec::new(0, ROOT, Role::Dialog, "Save changes?", (0, 0, 420, 180)),
            Spec::new(
                1,
                under(0),
                Role::Label,
                "Your changes will be lost if you do not save.",
                (20, 40, 380, 40),
            ),
            button(2, "Cancel", 140),
            button(3, "Discard", 230),
            button(4, "Save", 320),
        ],
    )
}

pub fn firefox_form() -> A11ySnapshot {
    let mut email = Spec::new(3, under(0), Role::Entry, "Email", (40, 120, 320, 32));
    email.states = &[
        NodeState::Enabled,
        NodeState::Focusable,
        NodeState::Editable,
        NodeState::Visible,
        NodeState::Showing,
    ];
    email.value = NodeValue::Text(screen("org.mozilla.firefox", "ada@example.org"));
    let mut password = Spec::new(
        4,
        under(0),
        Role::PasswordText,
        "Password",
        (40, 170, 320, 32),
    );
    password.states = email.states;
    password.value = NodeValue::Text(screen(
        "org.mozilla.firefox",
        "correct horse battery staple",
    ));
    password.sensitivity = Sensitivity::Secret;
    let mut remember = Spec::new(
        5,
        under(0),
        Role::CheckBox,
        "Remember me",
        (40, 220, 140, 24),
    );
    remember.states = &[
        NodeState::Enabled,
        NodeState::Focusable,
        NodeState::Checked,
        NodeState::Visible,
        NodeState::Showing,
    ];
    remember.actions = vec![NodeAction::Toggle];
    let mut sign_in = Spec::new(6, under(0), Role::Button, "Sign in", (40, 260, 120, 36));
    sign_in.states = FOCUSABLE;
    sign_in.actions = vec![NodeAction::Press];
    let mut forgot = Spec::new(
        7,
        under(0),
        Role::Link,
        "Forgot password?",
        (180, 268, 160, 20),
    );
    forgot.states = FOCUSABLE;
    forgot.actions = vec![NodeAction::Click];
    snapshot(
        "org.mozilla.firefox",
        TreeSource::Atspi,
        vec![
            Spec::new(
                0,
                ROOT,
                Role::Document,
                "Sign in to Example",
                (0, 0, 1000, 700),
            ),
            Spec::new(1, under(0), Role::Heading, "Sign in", (40, 40, 200, 40)),
            Spec::new(
                2,
                under(0),
                Role::Other(116),
                "Account form",
                (30, 100, 340, 220),
            ),
            email,
            password,
            remember,
            sign_in,
            forgot,
        ],
    )
}

pub fn quire_mail() -> A11ySnapshot {
    let mut archive = Spec::new(2, under(1), Role::Button, "Archive", (12, 8, 80, 32));
    archive.states = FOCUSABLE;
    archive.actions = vec![NodeAction::Press];
    archive.typed = TypedHint::Action(ActionName::parse("mail.thread.archive").unwrap());
    let thread = |id, name, y| {
        let mut s = Spec::new(id, under(3), Role::ListItem, name, (0, y, 400, 56));
        s.states = &[
            NodeState::Enabled,
            NodeState::Focusable,
            NodeState::Visible,
            NodeState::Showing,
        ];
        s.actions = vec![NodeAction::Activate];
        s
    };
    let first = thread(4, "Lisbon receipts, March", 0);
    let second = thread(5, "Re: invoice 2291", 56);
    let mut third = thread(6, "Lunch on Friday?", 112);
    third.states = &[
        NodeState::Enabled,
        NodeState::Focusable,
        NodeState::Selected,
        NodeState::Visible,
        NodeState::Showing,
    ];
    snapshot(
        "org.quire.Mail",
        TreeSource::QuireApp,
        vec![
            Spec::new(0, ROOT, Role::Window, "Inbox", (0, 0, 900, 600)),
            Spec::new(1, under(0), Role::ToolBar, "Toolbar", (0, 0, 900, 48)),
            archive,
            Spec::new(3, under(0), Role::List, "Threads", (0, 48, 400, 552)),
            first,
            second,
            third,
        ],
    )
}
