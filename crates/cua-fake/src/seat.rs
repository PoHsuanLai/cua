//! A seat that records what it is asked to do.

use cua_desktop::{EiSocket, LeaseGrant, LeaseId, SeatIsolation, Stamp};
use porter_core::Count;
use seat_input::{
    AgentSeat, Emulation, InputOp, InputOrigin, InputReceipt, Keymap, SeatCaps, SeatError,
};
use std::sync::Mutex;

/// What happened to a fake seat, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SeatEvent {
    /// A lease was attached.
    Attached(LeaseId),
    /// Operations were delivered.
    Performed(LeaseId, Vec<InputOp>),
    /// Emulation changed.
    Emulation(LeaseId, Emulation),
    /// A lease was detached.
    Detached(LeaseId),
}

/// An [`AgentSeat`] that records every call and can be told to fail the next `perform`.
#[derive(Debug)]
pub struct FakeSeat {
    caps: SeatCaps,
    keymap: Keymap,
    at: Stamp,
    log: Mutex<Vec<SeatEvent>>,
    fail_next: Mutex<Option<SeatError>>,
    emulation: Mutex<Emulation>,
}

impl FakeSeat {
    /// A seat of this isolation whose receipts are stamped `at`.
    pub fn new(isolation: SeatIsolation, at: Stamp) -> Self {
        Self {
            caps: SeatCaps::for_isolation(isolation),
            keymap: Keymap::default(),
            at,
            log: Mutex::new(Vec::new()),
            fail_next: Mutex::new(None),
            emulation: Mutex::new(Emulation::Running),
        }
    }

    /// Makes the next `perform` fail with `error`.
    pub fn fail_next(&self, error: SeatError) {
        *self.fail_next.lock().expect("fake seat lock") = Some(error);
    }

    /// Everything that happened, in order.
    pub fn events(&self) -> Vec<SeatEvent> {
        self.log.lock().expect("fake seat lock").clone()
    }

    fn push(&self, event: SeatEvent) {
        self.log.lock().expect("fake seat lock").push(event);
    }
}

impl AgentSeat for FakeSeat {
    fn caps(&self) -> SeatCaps {
        self.caps.clone()
    }

    fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    async fn attach(&self, lease: &LeaseGrant, socket: EiSocket) -> Result<(), SeatError> {
        drop(socket);
        self.push(SeatEvent::Attached(lease.id));
        Ok(())
    }

    async fn perform(&self, lease: LeaseId, ops: &[InputOp]) -> Result<InputReceipt, SeatError> {
        if let Some(error) = self.fail_next.lock().expect("fake seat lock").take() {
            return Err(error);
        }
        if *self.emulation.lock().expect("fake seat lock") == Emulation::Stopped {
            return Err(SeatError::Suspended);
        }
        self.push(SeatEvent::Performed(lease, ops.to_vec()));
        let origin = match self.caps.isolation {
            SeatIsolation::AgentSeat => InputOrigin::AgentSeat(lease),
            SeatIsolation::SharedSeat => InputOrigin::SharedSeatViaEi,
        };
        Ok(InputReceipt {
            lease,
            ops: Count(u32::try_from(ops.len()).unwrap_or(u32::MAX)),
            text_path: None,
            origin,
            at: self.at,
        })
    }

    async fn emulation(&self, lease: LeaseId, state: Emulation) -> Result<(), SeatError> {
        *self.emulation.lock().expect("fake seat lock") = state;
        self.push(SeatEvent::Emulation(lease, state));
        Ok(())
    }

    async fn detach(&self, lease: LeaseId) -> Result<(), SeatError> {
        self.push(SeatEvent::Detached(lease));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{Button, Coord, Point};
    use seat_input::Press;
    use std::future::Future;
    use std::pin::pin;
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

    fn ops() -> Vec<InputOp> {
        vec![
            InputOp::MoveTo(Point::new(Coord(10), Coord(20))),
            InputOp::Button {
                button: Button::Left,
                dir: Press::Down,
            },
            InputOp::Button {
                button: Button::Left,
                dir: Press::Up,
            },
        ]
    }

    #[test]
    fn the_fake_seat_records_what_it_delivers_and_stamps_the_receipt() {
        let seat = FakeSeat::new(SeatIsolation::SharedSeat, Stamp(77));
        let receipt = ready(seat.perform(LeaseId(1), &ops())).unwrap();
        assert_eq!(receipt.ops, Count(3));
        assert_eq!(receipt.at, Stamp(77));
        assert_eq!(receipt.origin, InputOrigin::SharedSeatViaEi);
        assert_eq!(seat.events(), vec![SeatEvent::Performed(LeaseId(1), ops())]);
    }

    #[test]
    fn a_stopped_seat_delivers_nothing_and_a_scripted_failure_happens_once() {
        let seat = FakeSeat::new(SeatIsolation::AgentSeat, Stamp(0));
        ready(seat.emulation(LeaseId(1), Emulation::Stopped)).unwrap();
        assert_eq!(
            ready(seat.perform(LeaseId(1), &ops())),
            Err(SeatError::Suspended)
        );
        ready(seat.emulation(LeaseId(1), Emulation::Running)).unwrap();
        seat.fail_next(SeatError::TargetGone);
        assert_eq!(
            ready(seat.perform(LeaseId(1), &ops())),
            Err(SeatError::TargetGone)
        );
        assert!(ready(seat.perform(LeaseId(1), &ops())).is_ok());
        let delivered = seat
            .events()
            .into_iter()
            .filter(|e| matches!(e, SeatEvent::Performed(..)))
            .count();
        assert_eq!(delivered, 1, "refused calls are not recorded as delivered");
    }
}
