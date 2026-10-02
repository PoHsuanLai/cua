//! A desktop with the windows it was given and the events it was told to raise.

use crate::eis::FakeEis;
use cua_desktop::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, EiOrigin, GlowState,
    LeaseControl, LeaseGrant, LeaseId, LeaseRequest, Leased, SeatIsolation, Suspend, ToplevelInfo,
};
use std::collections::VecDeque;
use std::sync::Mutex;

/// What a fake desktop was asked to do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopCall {
    /// A lease was granted.
    Leased(LeaseId),
    /// A lease was released.
    Released(LeaseId),
    /// A lease was suspended.
    Suspended(LeaseId, Suspend),
    /// A lease was resumed.
    Resumed(LeaseId),
    /// The glow changed.
    Glow(LeaseId, GlowState),
}

/// A [`Desktop`] and [`LeaseControl`] over a list of windows, with the capabilities of a backend.
#[derive(Debug)]
pub struct FakeDesktop {
    caps: BackendCaps,
    windows: Vec<ToplevelInfo>,
    events: Mutex<VecDeque<DesktopEvent>>,
    calls: Mutex<Vec<DesktopCall>>,
    next_lease: Mutex<u64>,
}

impl FakeDesktop {
    /// A desktop of this backend over these windows.
    pub fn new(kind: BackendKind, windows: Vec<ToplevelInfo>) -> Self {
        Self {
            caps: BackendCaps::of(kind),
            windows,
            events: Mutex::new(VecDeque::new()),
            calls: Mutex::new(Vec::new()),
            next_lease: Mutex::new(1),
        }
    }

    /// Queues an event for `next_event`.
    pub fn raise(&self, event: DesktopEvent) {
        self.events
            .lock()
            .expect("fake desktop lock")
            .push_back(event);
    }

    /// What it was asked to do, in order.
    pub fn calls(&self) -> Vec<DesktopCall> {
        self.calls.lock().expect("fake desktop lock").clone()
    }

    fn note(&self, call: DesktopCall) {
        self.calls.lock().expect("fake desktop lock").push(call);
    }
}

impl Desktop for FakeDesktop {
    fn caps(&self) -> BackendCaps {
        self.caps
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        Ok(self.windows.clone())
    }

    async fn next_event(&self) -> DesktopEvent {
        let next = self.events.lock().expect("fake desktop lock").pop_front();
        match next {
            Some(event) => event,
            None => std::future::pending().await,
        }
    }
}

impl LeaseControl for FakeDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        let info = self
            .windows
            .iter()
            .find(|w| w.ident == req.window)
            .ok_or(DesktopError::NoSuchWindow)?;
        req.check(info)?;
        let mode = cua_run::mode_support(&self.caps, req.mode);
        if let cua_bus::ModeSupport::Unsupported(_) = mode {
            return Err(DesktopError::ModeUnsupported(req.mode));
        }
        let (_server, socket) =
            FakeEis::pair().map_err(|e| DesktopError::Protocol(e.to_string()))?;
        let id = {
            let mut next = self.next_lease.lock().expect("fake desktop lock");
            let id = LeaseId(*next);
            *next += 1;
            id
        };
        let ei = match self.caps.seat {
            SeatIsolation::AgentSeat => EiOrigin::LeaseSocket,
            SeatIsolation::SharedSeat => EiOrigin::SenderSocket,
        };
        self.note(DesktopCall::Leased(id));
        Ok(Leased {
            grant: LeaseGrant {
                id,
                window: info.clone(),
                seat: self.caps.seat,
                ei,
            },
            socket,
        })
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.note(DesktopCall::Released(lease));
        Ok(())
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        self.note(DesktopCall::Suspended(lease, why));
        Ok(())
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.note(DesktopCall::Resumed(lease));
        Ok(())
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        self.note(DesktopCall::Glow(lease, glow));
        Ok(())
    }
}
