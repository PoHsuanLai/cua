//! The EI client over reis: the stock `GetSenderSocket` socket or the fork's lease socket.
//! Measured on stock cosmic-comp 1.8: the handshake offers `ei_pointer_absolute`, `ei_button`,
//! `ei_scroll`, `ei_keyboard` and `ei_text`; the keyboard device sends a 35 KB xkb keymap; the
//! absolute regions are global logical coordinates, one per output; `start_emulating` takes
//! `(last_serial, sequence)`.

use crate::caps::SeatCaps;
use crate::keymap::Keymap;
use crate::op::InputOp;
use crate::receipt::InputReceipt;
use crate::seat::{AgentSeat, Emulation, SeatError};
use cua_desktop::{EiSocket, LeaseGrant, LeaseId, SeatIsolation};

/// A seat over one EIS connection.
#[derive(Debug)]
pub struct EiSeat {
    caps: SeatCaps,
    keymap: Keymap,
    context: Option<reis::ei::Context>,
}

impl EiSeat {
    /// A seat that is not connected yet; `attach` connects it.
    pub fn new(isolation: SeatIsolation) -> Self {
        Self {
            caps: SeatCaps::for_isolation(isolation),
            keymap: Keymap::default(),
            context: None,
        }
    }

    /// Whether the seat holds an EIS connection.
    pub fn is_connected(&self) -> bool {
        self.context.is_some()
    }
}

impl AgentSeat for EiSeat {
    fn caps(&self) -> SeatCaps {
        self.caps.clone()
    }

    fn keymap(&self) -> &Keymap {
        &self.keymap
    }

    async fn attach(&self, lease: &LeaseGrant, socket: EiSocket) -> Result<(), SeatError> {
        let _ = (lease, socket);
        todo!(
            "handshake as a sender, bind seat devices, read the keymap, start_emulating (ei_seat_against_fake_eis)"
        )
    }

    async fn perform(&self, lease: LeaseId, ops: &[InputOp]) -> Result<InputReceipt, SeatError> {
        let _ = (lease, ops);
        todo!(
            "one frame per op: pointer_absolute through the RegionMap, button, scroll_discrete, key, text"
        )
    }

    async fn emulation(&self, lease: LeaseId, state: Emulation) -> Result<(), SeatError> {
        let _ = (lease, state);
        todo!("start_emulating or stop_emulating; perform answers Suspended while stopped")
    }

    async fn detach(&self, lease: LeaseId) -> Result<(), SeatError> {
        let _ = lease;
        todo!("stop_emulating, disconnect (held keys are released by the server)")
    }
}
