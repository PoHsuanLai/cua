//! What `plan` produces: structure, not timing (the size of a `Gap` is the fill's to choose).

use cua_action::{
    Button, Chord, ClickCount, Coord, CuaAction, Length, Modifier, NodeId, Notches, Point, Repeat,
    ScrollBy, ScrollDir, Size, Target, WindowSpace,
};
use cua_desktop::SeatIsolation;
use seat_input::{
    InputOp, Keycode, Keymap, KeymapEntry, PlanError, PlannedAction, Press, Produces,
    ResolvedPoints, SeatCaps, ShiftLevel, plan,
};
use std::collections::BTreeSet;

fn at(x: u32, y: u32) -> Point<WindowSpace> {
    Point::new(Coord(x), Coord(y))
}

fn window() -> Size<WindowSpace> {
    Size::new(Coord(800), Coord(600))
}

fn caps() -> SeatCaps {
    SeatCaps::for_isolation(SeatIsolation::SharedSeat)
}

fn us_keymap() -> Keymap {
    let key = |name: &str, code: u32| KeymapEntry {
        produces: Produces::Named(name.into()),
        code: Keycode(code),
        level: ShiftLevel::Plain,
    };
    let mut entries = vec![
        key("Control", 29),
        key("Shift", 42),
        key("Alt", 56),
        key("Meta", 125),
    ];
    entries.extend(
        "abcdefghijklmnopqrstuvwxyz"
            .chars()
            .zip(30u32..)
            .map(|(c, code)| KeymapEntry {
                produces: Produces::Char(c),
                code: Keycode(code),
                level: ShiftLevel::Plain,
            }),
    );
    Keymap { entries }
}

fn click(button: Button, count: ClickCount, mods: &[Modifier]) -> PlannedAction {
    let point = at(10, 20);
    PlannedAction {
        action: CuaAction::Click {
            at: Target::Point(point),
            button,
            count,
            mods: mods.iter().copied().collect::<BTreeSet<_>>(),
        },
        at: ResolvedPoints::One(point),
        window: window(),
    }
}

/// The operations without the gaps: only the order of moves, buttons and keys is asserted.
fn without_gaps(ops: Vec<InputOp>) -> Vec<InputOp> {
    ops.into_iter()
        .filter(|op| !matches!(op, InputOp::Gap(_)))
        .collect()
}

fn button(button: Button, dir: Press) -> InputOp {
    InputOp::Button { button, dir }
}

#[test]
fn plan_click_double_right_mods_table() {
    struct Row {
        name: &'static str,
        action: PlannedAction,
        want: Vec<InputOp>,
    }
    let ctrl_down = InputOp::Key {
        key: Keycode(29),
        dir: Press::Down,
    };
    let ctrl_up = InputOp::Key {
        key: Keycode(29),
        dir: Press::Up,
    };
    let rows = [
        Row {
            name: "left click",
            action: click(Button::Left, ClickCount::One, &[]),
            want: vec![
                InputOp::MoveTo(at(10, 20)),
                button(Button::Left, Press::Down),
                button(Button::Left, Press::Up),
            ],
        },
        Row {
            name: "double click",
            action: click(Button::Left, ClickCount::Two, &[]),
            want: vec![
                InputOp::MoveTo(at(10, 20)),
                button(Button::Left, Press::Down),
                button(Button::Left, Press::Up),
                button(Button::Left, Press::Down),
                button(Button::Left, Press::Up),
            ],
        },
        Row {
            name: "right click",
            action: click(Button::Right, ClickCount::One, &[]),
            want: vec![
                InputOp::MoveTo(at(10, 20)),
                button(Button::Right, Press::Down),
                button(Button::Right, Press::Up),
            ],
        },
        Row {
            name: "ctrl click: the modifier brackets the click",
            action: click(Button::Left, ClickCount::One, &[Modifier::Ctrl]),
            want: vec![
                InputOp::MoveTo(at(10, 20)),
                ctrl_down,
                button(Button::Left, Press::Down),
                button(Button::Left, Press::Up),
                ctrl_up,
            ],
        },
    ];
    for row in rows {
        let ops = without_gaps(plan(&row.action, &us_keymap(), &caps()).expect(row.name));
        assert_eq!(ops, row.want, "{}", row.name);
    }
}

#[test]
fn plan_drag_emits_down_moves_up() {
    let drag = PlannedAction {
        action: CuaAction::Drag {
            from: Target::Point(at(10, 10)),
            to: Target::Point(at(200, 100)),
            button: Button::Left,
        },
        at: ResolvedPoints::Drag {
            from: at(10, 10),
            to: at(200, 100),
        },
        window: window(),
    };
    let ops = without_gaps(plan(&drag, &us_keymap(), &caps()).expect("a drag"));
    assert_eq!(ops.first(), Some(&InputOp::MoveTo(at(10, 10))));
    assert_eq!(ops.get(1), Some(&button(Button::Left, Press::Down)));
    assert_eq!(ops.last(), Some(&button(Button::Left, Press::Up)));
    let moves: Vec<_> = ops
        .iter()
        .filter(|op| matches!(op, InputOp::MoveTo(_)))
        .collect();
    assert!(moves.len() >= 2, "it moves between press and release");
    assert_eq!(
        ops[ops.len() - 2],
        InputOp::MoveTo(at(200, 100)),
        "and ends on the target"
    );
}

#[test]
fn plan_rejects_point_outside_window_and_what_is_not_input() {
    let outside = click(Button::Left, ClickCount::One, &[]);
    let outside = PlannedAction {
        at: ResolvedPoints::One(at(900, 10)),
        ..outside
    };
    assert_eq!(
        plan(&outside, &us_keymap(), &caps()),
        Err(PlanError::OutsideWindow(at(900, 10)))
    );
    let observe = PlannedAction {
        action: CuaAction::Observe,
        at: ResolvedPoints::None,
        window: window(),
    };
    assert_eq!(
        plan(&observe, &us_keymap(), &caps()),
        Err(PlanError::NotAnInput)
    );
}

#[test]
fn unmappable_char_errors_named_when_only_the_key_code_path_exists() {
    let typed = PlannedAction {
        action: CuaAction::Type {
            text: cua_action::TypedText::new("你").unwrap(),
        },
        at: ResolvedPoints::None,
        window: window(),
    };
    assert_eq!(
        plan(&typed, &us_keymap(), &caps()),
        Err(PlanError::Unmappable('你'))
    );
}

#[test]
fn a_chord_presses_modifiers_then_the_key_and_releases_in_reverse() {
    let chord = Chord::new(
        BTreeSet::from([Modifier::Ctrl]),
        keyboard_types::Key::Character("s".into()),
    )
    .unwrap();
    let action = PlannedAction {
        action: CuaAction::Key {
            chord,
            repeat: Repeat::ONCE,
        },
        at: ResolvedPoints::None,
        window: window(),
    };
    let ops = without_gaps(plan(&action, &us_keymap(), &caps()).expect("a chord"));
    let k = |code, dir| InputOp::Key {
        key: Keycode(code),
        dir,
    };
    assert_eq!(
        ops,
        vec![
            k(29, Press::Down),
            k(48, Press::Down),
            k(48, Press::Up),
            k(29, Press::Up)
        ]
    );
}

fn at_target(
    action: CuaAction<WindowSpace>,
    given: ResolvedPoints,
    window: Size<WindowSpace>,
) -> PlannedAction {
    PlannedAction {
        action,
        at: given,
        window,
    }
}

fn size(w: u32, h: u32) -> Size<WindowSpace> {
    Size::new(Coord(w), Coord(h))
}

#[test]
fn the_centre_target_is_the_centre_of_the_window_it_is_planned_for() {
    // (window, the centre it resolves to): half of each side, rounded down.
    let table = [
        (size(800, 600), at(400, 300)),
        (size(801, 601), at(400, 300)),
        (size(3, 5), at(1, 2)),
        (size(2, 2), at(1, 1)),
        (size(1, 1), at(0, 0)),
        (size(1920, 1), at(960, 0)),
    ];
    let scroll = |window| {
        at_target(
            CuaAction::Scroll {
                at: Target::Centre,
                dir: ScrollDir::Down,
                by: ScrollBy::Notches(Notches(3)),
            },
            ResolvedPoints::None,
            window,
        )
    };
    for (window, centre) in table {
        assert_eq!(seat_input::centre_of(window), centre, "{window:?}");
        let ops = plan(&scroll(window), &us_keymap(), &caps()).expect("a scroll");
        assert_eq!(
            ops,
            vec![
                InputOp::MoveTo(centre),
                InputOp::Scroll {
                    dir: ScrollDir::Down,
                    notches: Notches(3)
                }
            ],
            "{window:?}"
        );
    }
    // The same for the other pointer actions, and the caller's `at` never overrides a centre.
    let click = at_target(
        CuaAction::Click {
            at: Target::Centre,
            button: Button::Left,
            count: ClickCount::One,
            mods: BTreeSet::new(),
        },
        ResolvedPoints::One(at(5, 5)),
        window(),
    );
    assert_eq!(
        without_gaps(plan(&click, &us_keymap(), &caps()).expect("click"))[0],
        InputOp::MoveTo(at(400, 300))
    );
    let hover = at_target(
        CuaAction::MoveTo { at: Target::Centre },
        ResolvedPoints::None,
        window(),
    );
    assert_eq!(
        plan(&hover, &us_keymap(), &caps()),
        Ok(vec![InputOp::MoveTo(at(400, 300))])
    );
    let drag = at_target(
        CuaAction::Drag {
            from: Target::Centre,
            to: Target::Point(at(700, 500)),
            button: Button::Left,
        },
        ResolvedPoints::Drag {
            from: at(1, 1),
            to: at(700, 500),
        },
        window(),
    );
    let ops = without_gaps(plan(&drag, &us_keymap(), &caps()).expect("drag"));
    assert_eq!(ops[0], InputOp::MoveTo(at(400, 300)));
    assert_eq!(ops[ops.len() - 2], InputOp::MoveTo(at(700, 500)));
}

#[test]
fn nodes_come_from_the_caller_and_every_point_is_checked_against_the_window() {
    let click_at = |target, given| {
        at_target(
            CuaAction::Click {
                at: target,
                button: Button::Left,
                count: ClickCount::One,
                mods: BTreeSet::new(),
            },
            given,
            window(),
        )
    };
    let node = Target::Node(NodeId(7));
    let moved = |p| Ok(vec![InputOp::MoveTo(p)]);
    let first = |action: &PlannedAction| {
        plan(action, &us_keymap(), &caps())
            .map(|ops| without_gaps(ops).into_iter().take(1).collect::<Vec<_>>())
    };
    assert_eq!(
        first(&click_at(node, ResolvedPoints::One(at(9, 9)))),
        moved(at(9, 9))
    );
    assert_eq!(
        first(&click_at(node, ResolvedPoints::None)),
        Err(PlanError::Unresolved)
    );
    // The edge: the last pixel is inside, the one past it is not.
    let last = at(799, 599);
    assert_eq!(
        first(&click_at(Target::Point(last), ResolvedPoints::None)),
        moved(last)
    );
    for outside in [at(800, 0), at(0, 600)] {
        assert_eq!(
            first(&click_at(Target::Point(outside), ResolvedPoints::None)),
            Err(PlanError::OutsideWindow(outside))
        );
    }
}

#[test]
fn a_scroll_by_distance_is_whole_notches_of_at_least_one() {
    let by = |px: u32| {
        let action = at_target(
            CuaAction::Scroll {
                at: Target::Centre,
                dir: ScrollDir::Up,
                by: ScrollBy::Distance(Length::new(Coord(px))),
            },
            ResolvedPoints::None,
            window(),
        );
        match plan(&action, &us_keymap(), &caps()).expect("scroll").pop() {
            Some(InputOp::Scroll { notches, .. }) => notches,
            other => panic!("{other:?}"),
        }
    };
    let table = [
        (0, 1),
        (1, 1),
        (40, 1),
        (41, 2),
        (400, 10),
        (u32::MAX, u16::MAX),
    ];
    for (px, want) in table {
        assert_eq!(by(px), Notches(want), "{px} px");
    }
}

#[test]
fn typing_goes_by_the_seats_text_path() {
    let typed = |text: &str| PlannedAction {
        action: CuaAction::Type {
            text: cua_action::TypedText::new(text).unwrap(),
        },
        at: ResolvedPoints::None,
        window: window(),
    };
    let k = |code, dir| InputOp::Key {
        key: Keycode(code),
        dir,
    };
    // Key codes: letters from 30, as the test keymap lays them out.
    let keys = plan(&typed("ab"), &us_keymap(), &caps()).expect("keys");
    assert_eq!(
        keys,
        vec![
            k(30, Press::Down),
            k(30, Press::Up),
            k(31, Press::Down),
            k(31, Press::Up)
        ]
    );
    // The fork's agent text input takes the whole text, Unicode included.
    let fork = SeatCaps::for_isolation(SeatIsolation::AgentSeat);
    let whole = plan(&typed("你好"), &us_keymap(), &fork).expect("text");
    assert_eq!(
        whole,
        vec![InputOp::Text(cua_action::TypedText::new("你好").unwrap())]
    );
    // A shifted level brackets the stroke with Shift; one unmappable character refuses the lot.
    let mut keymap = us_keymap();
    keymap.entries.push(KeymapEntry {
        produces: Produces::Char('A'),
        code: Keycode(30),
        level: ShiftLevel::Shifted,
    });
    let shifted = plan(&typed("A"), &keymap, &caps()).expect("shifted");
    assert_eq!(
        shifted,
        vec![
            k(42, Press::Down),
            k(30, Press::Down),
            k(30, Press::Up),
            k(42, Press::Up)
        ]
    );
    assert_eq!(
        plan(&typed("ab你"), &keymap, &caps()),
        Err(PlanError::Unmappable('你'))
    );
}

#[test]
fn what_is_not_input_and_unknown_keys_are_named() {
    for action in [
        CuaAction::Wait {
            for_ms: cua_action::WaitMs::new(10).unwrap(),
        },
        CuaAction::Observe,
    ] {
        let planned = at_target(action, ResolvedPoints::None, window());
        assert_eq!(
            plan(&planned, &us_keymap(), &caps()),
            Err(PlanError::NotAnInput)
        );
    }
    let hyper = at_target(
        CuaAction::Key {
            chord: Chord::new(BTreeSet::new(), keyboard_types::Key::Hyper).unwrap(),
            repeat: Repeat::ONCE,
        },
        ResolvedPoints::None,
        window(),
    );
    assert_eq!(
        plan(&hyper, &us_keymap(), &caps()),
        Err(PlanError::UnknownKey("Hyper".into()))
    );
}

#[test]
fn the_keymap_finds_characters_and_named_keys() {
    let keymap = us_keymap();
    let table: [(&str, Option<Keycode>); 5] = [
        ("a", Some(Keycode(30))),
        ("S", Some(Keycode(48))),
        ("ab", None),
        ("你", None),
        ("", None),
    ];
    for (text, want) in table {
        let key = keyboard_types::Key::Character(text.into());
        assert_eq!(keymap.code_of(&key), want, "{text:?}");
    }
    assert_eq!(
        keymap.code_of(&keyboard_types::Key::Control),
        Some(Keycode(29))
    );
    assert_eq!(
        keymap.code_of(&keyboard_types::Key::Meta),
        Some(Keycode(125))
    );
    assert_eq!(keymap.code_of(&keyboard_types::Key::Enter), None);
    assert_eq!(
        keymap.stroke_for('z'),
        Some((Keycode(55), ShiftLevel::Plain))
    );
    assert_eq!(keymap.stroke_for('Z'), None);
}
