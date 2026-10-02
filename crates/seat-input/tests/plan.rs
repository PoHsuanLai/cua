//! What `plan` must produce. It is frozen with a `todo!()` body, so each test is ignored with
//! the work that closes it and carries the real assertions (structure, not timing: the size of a
//! `Gap` is the fill's to choose).

use cua_action::{
    Button, Chord, ClickCount, Coord, CuaAction, Modifier, Point, Repeat, Size, Target, WindowSpace,
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
#[ignore = "closes with the seat-input fill (plan)"]
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
#[ignore = "closes with the seat-input fill (plan)"]
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
#[ignore = "closes with the seat-input fill (plan)"]
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
#[ignore = "closes with the seat-input fill (plan)"]
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
#[ignore = "closes with the seat-input fill (plan)"]
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
            k(49, Press::Down),
            k(49, Press::Up),
            k(29, Press::Up)
        ]
    );
}
