//! The EI client over reis: the stock `GetSenderSocket` socket or the fork's lease socket.
//! Measured on stock cosmic-comp 1.8: the handshake offers `ei_pointer_absolute`, `ei_button`,
//! `ei_scroll`, `ei_keyboard` and `ei_text`; the keyboard device sends a 35 KB xkb keymap; the
//! absolute regions are global logical coordinates, one per output; `start_emulating` takes
//! `(last_serial, sequence)`.

mod actor;
mod conn;

use crate::caps::SeatCaps;
use crate::keymap::Keymap;
use crate::op::InputOp;
use crate::receipt::{InputOrigin, InputReceipt};
use crate::seat::{AgentSeat, Emulation, SeatError};
use actor::{Cmd, Link};
use cua_desktop::{EiSocket, LeaseGrant, LeaseId, SeatIsolation};
use porter_core::Count;
use std::sync::{Arc, PoisonError, RwLock};
use tokio::sync::Mutex;

/// A seat over one EIS connection. The connection lives on a thread of its own (reis proxies
/// are not `Send`).
#[derive(Debug)]
pub struct EiSeat {
    caps: SeatCaps,
    keymap: RwLock<Arc<Keymap>>,
    link: Mutex<Option<Link>>,
}

impl EiSeat {
    /// A seat that is not connected yet; `attach` connects it.
    pub fn new(isolation: SeatIsolation) -> Self {
        Self {
            caps: SeatCaps::for_isolation(isolation),
            keymap: RwLock::new(Arc::new(Keymap::default())),
            link: Mutex::new(None),
        }
    }

    /// Whether the seat holds an EIS connection. A seat busy in a call counts as connected.
    pub fn is_connected(&self) -> bool {
        self.link.try_lock().map_or(true, |l| l.is_some())
    }

    fn origin(&self, lease: LeaseId) -> InputOrigin {
        match self.caps.isolation {
            SeatIsolation::AgentSeat => InputOrigin::AgentSeat(lease),
            SeatIsolation::SharedSeat => InputOrigin::SharedSeatViaEi,
        }
    }
}

/// The link of `lease`, or `NotConnected`.
fn of_lease(slot: &Option<Link>, lease: LeaseId) -> Result<&Link, SeatError> {
    slot.as_ref()
        .filter(|l| l.lease == lease)
        .ok_or(SeatError::NotConnected)
}

impl AgentSeat for EiSeat {
    fn caps(&self) -> SeatCaps {
        self.caps.clone()
    }

    fn keymap(&self) -> Arc<Keymap> {
        self.keymap
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    async fn attach(&self, lease: &LeaseGrant, socket: EiSocket) -> Result<(), SeatError> {
        let (link, keymap) = actor::spawn(lease, socket.0).await?;
        *self.keymap.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(keymap);
        // The old link, if any, is dropped here: its thread says goodbye and ends.
        *self.link.lock().await = Some(link);
        Ok(())
    }

    async fn perform(&self, lease: LeaseId, ops: &[InputOp]) -> Result<InputReceipt, SeatError> {
        let mut slot = self.link.lock().await;
        let result = of_lease(&slot, lease)?
            .ask(|reply| Cmd::Perform(ops.to_vec(), reply))
            .await;
        if result == Err(SeatError::NotConnected) {
            slot.take();
        }
        Ok(InputReceipt {
            lease,
            ops: Count(u32::try_from(ops.len()).unwrap_or(u32::MAX)),
            text_path: None,
            origin: self.origin(lease),
            at: result?,
        })
    }

    async fn emulation(&self, lease: LeaseId, state: Emulation) -> Result<(), SeatError> {
        let slot = self.link.lock().await;
        of_lease(&slot, lease)?
            .ask(|reply| Cmd::Emulation(state, reply))
            .await
    }

    async fn detach(&self, lease: LeaseId) -> Result<(), SeatError> {
        let mut slot = self.link.lock().await;
        let result = of_lease(&slot, lease)?.ask(Cmd::Detach).await;
        slot.take();
        result
    }
}
