//! Every wire type of the seat survives serde and pins the JSON other crates read.

use cua_action::{
    Button, Coord, CuaAction, Notches, Point, ScrollDir, Size, Summary, TypedText, WindowSpace,
};
use cua_desktop::{Ms, SeatIsolation};
use seat_input::{
    InputOp, Keycode, Keymap, KeymapEntry, PlanError, PlannedAction, Press, Produces,
    ResolvedPoints, ScrollKind, SeatCaps, SeatError, ShiftLevel, TextPath,
};

fn round_trip<T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
    value: &T,
) -> String {
    let json = serde_json::to_string(value).unwrap();
    assert_eq!(&serde_json::from_str::<T>(&json).unwrap(), value, "{json}");
    json
}

fn at(x: u32, y: u32) -> Point<WindowSpace> {
    Point::new(Coord(x), Coord(y))
}

#[test]
fn every_input_op_pins_its_json() {
    let table: Vec<(InputOp, &str)> = vec![
        (
            InputOp::MoveTo(at(10, 20)),
            r#"{"kind":"move_to","v":{"x":10,"y":20}}"#,
        ),
        (
            InputOp::Button {
                button: Button::Left,
                dir: Press::Down,
            },
            r#"{"kind":"button","v":{"button":"left","dir":"down"}}"#,
        ),
        (
            InputOp::Key {
                key: Keycode(29),
                dir: Press::Up,
            },
            r#"{"kind":"key","v":{"key":29,"dir":"up"}}"#,
        ),
        (
            InputOp::Text(TypedText::new("héllo").unwrap()),
            r#"{"kind":"text","v":"héllo"}"#,
        ),
        (
            InputOp::Scroll {
                dir: ScrollDir::Down,
                notches: Notches(3),
            },
            r#"{"kind":"scroll","v":{"dir":"down","notches":3}}"#,
        ),
        (InputOp::Gap(Ms(40)), r#"{"kind":"gap","v":40}"#),
    ];
    for (op, json) in table {
        assert_eq!(round_trip(&op), json);
    }
}

#[test]
fn planning_types_and_errors_round_trip() {
    let drag = ResolvedPoints::Drag {
        from: at(1, 2),
        to: at(30, 40),
    };
    assert_eq!(
        round_trip(&drag),
        r#"{"kind":"drag","v":{"from":{"x":1,"y":2},"to":{"x":30,"y":40}}}"#
    );
    round_trip(&ResolvedPoints::None);
    round_trip(&ResolvedPoints::One(at(5, 5)));
    let planned = PlannedAction {
        action: CuaAction::<WindowSpace>::Finish {
            outcome: cua_action::FinishOutcome::Done,
            summary: Summary::new("done").unwrap(),
            extracted: vec![],
        },
        at: ResolvedPoints::None,
        window: Size::new(Coord(800), Coord(600)),
    };
    round_trip(&planned);
    for error in [
        PlanError::OutsideWindow(at(900, 10)),
        PlanError::Unmappable('你'),
        PlanError::NotAnInput,
        PlanError::UnknownKey("Hyper".into()),
    ] {
        round_trip(&error);
    }
    assert_eq!(
        round_trip(&PlanError::NotAnInput),
        r#"{"kind":"not_an_input"}"#
    );
    for error in [
        SeatError::NotConnected,
        SeatError::Refused,
        SeatError::OutsideLease,
        SeatError::TargetGone,
        SeatError::Suspended,
        SeatError::Keymap("short".into()),
        SeatError::Protocol("serial".into()),
    ] {
        round_trip(&error);
    }
}

#[test]
fn caps_and_keymaps_round_trip() {
    let caps = SeatCaps::for_isolation(SeatIsolation::SharedSeat);
    assert_eq!(
        round_trip(&caps),
        r#"{"isolation":"shared_seat","text":["ei_keymap"],"scroll":"discrete"}"#
    );
    assert_eq!(round_trip(&ScrollKind::Smooth), r#""smooth""#);
    for path in [
        TextPath::EiKeymap,
        TextPath::EiText,
        TextPath::VirtualKeyboard,
        TextPath::AgentTextInput,
    ] {
        round_trip(&path);
    }
    let keymap = Keymap {
        entries: vec![
            KeymapEntry {
                produces: Produces::Char('a'),
                code: Keycode(30),
                level: ShiftLevel::Plain,
            },
            KeymapEntry {
                produces: Produces::Char('A'),
                code: Keycode(30),
                level: ShiftLevel::Shifted,
            },
            KeymapEntry {
                produces: Produces::Named("Enter".into()),
                code: Keycode(28),
                level: ShiftLevel::Plain,
            },
        ],
    };
    let json = round_trip(&keymap);
    assert!(
        json.starts_with(
            r#"{"entries":[{"produces":{"kind":"char","v":"a"},"code":30,"level":"plain"}"#
        ),
        "{json}"
    );
}
