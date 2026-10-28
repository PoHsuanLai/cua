//! `EiSeat` against `FakeEis`: the real reis client over a socketpair, no compositor.

use cua_action::{Button, Coord, Notches, Point, ScrollDir, TypedText};
use cua_desktop::{
    EiOrigin, GlobalCoord, GlobalRect, LeaseGrant, LeaseId, Ms, SeatIsolation, ToplevelInfo,
};
use cua_fake::{EisEvent, FakeEis, scene};
use porter_ids::Px;
use seat_ei::EiSeat;
use seat_input::{AgentSeat, Emulation, InputOp, InputOrigin, Keycode, Press, Produces, SeatError};

const LEASE: LeaseId = LeaseId(7);

fn grant(ei: EiOrigin, seat: SeatIsolation, x: i32, y: i32) -> LeaseGrant {
    let mut window: ToplevelInfo = scene::window();
    window.geometry = GlobalRect {
        x: GlobalCoord(x),
        y: GlobalCoord(y),
        w: Px(300),
        h: Px(200),
    };
    LeaseGrant {
        id: LEASE,
        window,
        seat,
        ei,
    }
}

fn at(x: u32, y: u32) -> InputOp {
    InputOp::MoveTo(Point::new(Coord(x), Coord(y)))
}

fn stock() -> LeaseGrant {
    grant(EiOrigin::SenderSocket, SeatIsolation::SharedSeat, 714, 148)
}

/// Runs `script` against a fresh seat while the fake serves, and returns what the fake saw.
async fn run<F, Fut>(grant: LeaseGrant, script: F) -> Vec<EisEvent>
where
    F: FnOnce(EiSeat, LeaseGrant) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    let (mut server, socket) = FakeEis::pair().expect("socketpair");
    let seat = EiSeat::new(grant.seat);
    let client = async {
        seat.attach(&grant, socket).await.expect("attach");
        script(seat, grant.clone()).await;
    };
    let (events, ()) = tokio::join!(server.serve(), client);
    events
}

#[tokio::test]
async fn ei_seat_against_fake_eis() {
    let events = run(stock(), |seat, _| async move {
        assert!(seat.is_connected());
        let keymap = seat.keymap();
        let a = keymap
            .entries
            .iter()
            .find(|e| e.produces == Produces::Char('a'))
            .expect("a is in the keymap the server sent");
        assert_eq!(a.code, Keycode(30));
        let receipt = seat
            .perform(
                LEASE,
                &[
                    at(100, 50),
                    InputOp::Button {
                        button: Button::Left,
                        dir: Press::Down,
                    },
                    InputOp::Button {
                        button: Button::Left,
                        dir: Press::Up,
                    },
                    InputOp::Gap(Ms(1)),
                    InputOp::Key {
                        key: Keycode(30),
                        dir: Press::Down,
                    },
                    InputOp::Key {
                        key: Keycode(30),
                        dir: Press::Up,
                    },
                    InputOp::Scroll {
                        dir: ScrollDir::Down,
                        notches: Notches(2),
                    },
                    InputOp::Scroll {
                        dir: ScrollDir::Left,
                        notches: Notches(1),
                    },
                ],
            )
            .await
            .expect("perform");
        assert_eq!(receipt.ops.0, 8);
        assert_eq!(receipt.origin, InputOrigin::SharedSeatViaEi);
        seat.detach(LEASE).await.expect("detach");
        assert!(!seat.is_connected());
    })
    .await;
    let starts = events
        .iter()
        .filter(|e| **e == EisEvent::StartEmulating)
        .count();
    let stops = events
        .iter()
        .filter(|e| **e == EisEvent::StopEmulating)
        .count();
    assert!(starts >= 2 && starts == stops, "{events:?}");
    let input: Vec<&EisEvent> = events
        .iter()
        .filter(|e| {
            !matches!(
                e,
                EisEvent::Handshake
                    | EisEvent::Bind
                    | EisEvent::StartEmulating
                    | EisEvent::StopEmulating
            )
        })
        .collect();
    assert_eq!(
        input,
        vec![
            &EisEvent::PointerAbsolute { x: 814, y: 198 },
            &EisEvent::Button {
                code: 0x110,
                dir: Press::Down
            },
            &EisEvent::Button {
                code: 0x110,
                dir: Press::Up
            },
            &EisEvent::Key {
                code: 30,
                dir: Press::Down
            },
            &EisEvent::Key {
                code: 30,
                dir: Press::Up
            },
            &EisEvent::ScrollDiscrete { x: 0, y: 240 },
            &EisEvent::ScrollDiscrete { x: -120, y: 0 },
        ]
    );
    assert_eq!(events.first(), Some(&EisEvent::Handshake));
    assert_eq!(events.get(1), Some(&EisEvent::Bind));
}

#[tokio::test]
async fn the_fork_seat_maps_points_by_identity() {
    let g = grant(EiOrigin::LeaseSocket, SeatIsolation::AgentSeat, 714, 148);
    let events = run(g, |seat, _| async move {
        let receipt = seat.perform(LEASE, &[at(100, 50)]).await.expect("perform");
        assert_eq!(receipt.origin, InputOrigin::AgentSeat(LEASE));
        seat.detach(LEASE).await.expect("detach");
    })
    .await;
    assert!(events.contains(&EisEvent::PointerAbsolute { x: 100, y: 50 }));
}

#[tokio::test]
async fn stopped_emulation_suspends_and_running_resumes() {
    let events = run(stock(), |seat, _| async move {
        seat.emulation(LEASE, Emulation::Stopped)
            .await
            .expect("stop");
        assert_eq!(
            seat.perform(LEASE, &[at(1, 1)]).await.map(|_| ()),
            Err(SeatError::Suspended)
        );
        seat.emulation(LEASE, Emulation::Running)
            .await
            .expect("run");
        seat.perform(LEASE, &[at(2, 3)]).await.expect("perform");
        seat.detach(LEASE).await.expect("detach");
    })
    .await;
    let pointer: Vec<_> = events
        .iter()
        .filter(|e| matches!(e, EisEvent::PointerAbsolute { .. }))
        .collect();
    assert_eq!(pointer, vec![&EisEvent::PointerAbsolute { x: 716, y: 151 }]);
    let stops = events
        .iter()
        .filter(|e| **e == EisEvent::StopEmulating)
        .count();
    let starts = events
        .iter()
        .filter(|e| **e == EisEvent::StartEmulating)
        .count();
    // Two devices, started twice and stopped twice (the second stop is the detach).
    assert_eq!((starts, stops), (4, 4), "{events:?}");
}

#[tokio::test]
async fn refusals_are_local_and_the_connection_survives() {
    let events = run(stock(), |seat, _| async move {
        // Beyond the 1280x800 output.
        assert_eq!(
            seat.perform(LEASE, &[at(900, 10)]).await.map(|_| ()),
            Err(SeatError::OutsideLease)
        );
        // Inside the output, but past the 300x200 window.
        assert_eq!(
            seat.perform(LEASE, &[at(400, 10)]).await.map(|_| ()),
            Err(SeatError::OutsideLease)
        );
        let text = InputOp::Text(TypedText::new("héllo").unwrap());
        assert_eq!(
            seat.perform(LEASE, &[text]).await.map(|_| ()),
            Err(SeatError::Refused)
        );
        assert_eq!(
            seat.perform(LeaseId(99), &[at(1, 1)]).await.map(|_| ()),
            Err(SeatError::NotConnected)
        );
        seat.perform(LEASE, &[at(1, 1)])
            .await
            .expect("still usable");
        seat.detach(LEASE).await.expect("detach");
        assert_eq!(
            seat.perform(LEASE, &[at(1, 1)]).await.map(|_| ()),
            Err(SeatError::NotConnected)
        );
    })
    .await;
    assert!(events.contains(&EisEvent::PointerAbsolute { x: 715, y: 149 }));
}

#[tokio::test]
async fn a_rebound_window_takes_the_pointer_to_where_it_is_now() {
    let moved = GlobalRect {
        x: GlobalCoord(20),
        y: GlobalCoord(30),
        w: Px(300),
        h: Px(200),
    };
    let events = run(stock(), |seat, _| async move {
        seat.rebind(LEASE, moved).await.expect("rebind");
        seat.perform(LEASE, &[at(5, 5)]).await.expect("perform");
        seat.detach(LEASE).await.expect("detach");
    })
    .await;
    assert!(
        events.contains(&EisEvent::PointerAbsolute { x: 25, y: 35 }),
        "{events:?}"
    );
}

#[tokio::test]
async fn a_second_lease_does_not_take_the_seat_from_the_first() {
    let (mut server, socket) = FakeEis::pair().expect("socketpair");
    let (_idle, second_socket) = FakeEis::pair().expect("socketpair");
    let first = stock();
    let mut second = stock();
    second.id = LeaseId(8);
    let seat = EiSeat::new(first.seat);
    let client = async {
        seat.attach(&first, socket).await.expect("attach");
        assert_eq!(
            seat.attach(&second, second_socket).await,
            Err(SeatError::Busy)
        );
        seat.perform(LEASE, &[at(5, 5)])
            .await
            .expect("the first lease still has its input");
        seat.detach(LEASE).await.expect("detach");
    };
    let (_events, ()) = tokio::join!(server.serve(), client);
}

#[tokio::test]
async fn dropping_the_seat_ends_the_server_run() {
    let events = run(stock(), |seat, _| async move {
        drop(seat);
    })
    .await;
    assert_eq!(events.first(), Some(&EisEvent::Handshake));
}

fn code_of_a(seat: &EiSeat) -> Keycode {
    seat.keymap()
        .entries
        .iter()
        .find(|e| e.produces == Produces::Char('a'))
        .expect("a is in the keymap")
        .code
}

#[tokio::test]
async fn a_later_attach_replaces_the_keymap_of_the_earlier_one() {
    let g = stock();
    let seat = EiSeat::new(g.seat);
    let (mut first, socket_one) = FakeEis::pair().expect("socketpair");
    let (second, socket_two) = FakeEis::pair().expect("socketpair");
    let mut second = second.with_a_at(40);
    let client = async {
        assert!(
            seat.keymap().entries.is_empty(),
            "empty before the first attach"
        );
        seat.attach(&g, socket_one).await.expect("attach");
        assert_eq!(code_of_a(&seat), Keycode(30));
        seat.attach(&g, socket_two).await.expect("re-attach");
        assert_eq!(code_of_a(&seat), Keycode(40), "the new connection's keymap");
        seat.detach(LEASE).await.expect("detach");
    };
    tokio::join!(first.serve(), second.serve(), client);
}

#[tokio::test]
async fn a_dropped_perform_stops_the_thread_and_lets_go_of_held_keys() {
    let key = |dir| InputOp::Key {
        key: Keycode(30),
        dir,
    };
    let events = run(stock(), |seat, _| async move {
        // A long pause after a key goes down: the deadline drops the call mid-batch.
        let long = [key(Press::Down), InputOp::Gap(Ms(60_000)), key(Press::Up)];
        let stopped = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            seat.perform(LEASE, &long),
        )
        .await;
        assert!(stopped.is_err(), "the call was dropped");
        // The next call is not stuck behind the abandoned one.
        seat.perform(LEASE, &[at(2, 3)]).await.expect("perform");
        seat.detach(LEASE).await.expect("detach");
    })
    .await;
    let input: Vec<&EisEvent> = events
        .iter()
        .filter(|e| matches!(e, EisEvent::Key { .. } | EisEvent::PointerAbsolute { .. }))
        .collect();
    assert_eq!(
        input,
        vec![
            &EisEvent::Key {
                code: 30,
                dir: Press::Down
            },
            &EisEvent::Key {
                code: 30,
                dir: Press::Up
            },
            &EisEvent::PointerAbsolute { x: 716, y: 151 },
        ],
        "the held key is released and the abandoned tail never runs"
    );
}
