//! A desktop with the windows it was given and the events it was told to raise.

use crate::eis::FakeEis;
use cua_desktop::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, EiOrigin, GlowState,
    LeaseControl, LeaseGrant, LeaseId, LeaseRequest, Leased, SeatIsolation, Suspend, ToplevelIdent,
    ToplevelInfo, ToplevelState,
};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::task::{Poll, Waker};

/// The events raised and not yet read, and who is waiting for the next one.
#[derive(Debug, Default)]
struct Events {
    queue: VecDeque<DesktopEvent>,
    waiting: Option<Waker>,
}

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
    /// The lease's window was asked to take the focus.
    Focused(LeaseId),
    /// The glow changed.
    Glow(LeaseId, GlowState),
}

/// A [`Desktop`] and [`LeaseControl`] over a list of windows, with the capabilities of a backend.
#[derive(Debug)]
pub struct FakeDesktop {
    caps: BackendCaps,
    windows: Mutex<Vec<ToplevelInfo>>,
    held: Mutex<Vec<(LeaseId, ToplevelIdent)>>,
    events: Mutex<Events>,
    calls: Mutex<Vec<DesktopCall>>,
    next_lease: Mutex<u64>,
}

impl FakeDesktop {
    /// A desktop of this backend over these windows.
    pub fn new(kind: BackendKind, windows: Vec<ToplevelInfo>) -> Self {
        Self {
            caps: BackendCaps::of(kind),
            windows: Mutex::new(windows),
            held: Mutex::new(Vec::new()),
            events: Mutex::new(Events::default()),
            calls: Mutex::new(Vec::new()),
            next_lease: Mutex::new(1),
        }
    }

    /// Queues an event for `next_event`.
    pub fn raise(&self, event: DesktopEvent) {
        let mut events = self.events.lock().expect("fake desktop lock");
        events.queue.push_back(event);
        if let Some(waker) = events.waiting.take() {
            waker.wake();
        }
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
        Ok(self.windows.lock().expect("fake desktop lock").clone())
    }

    async fn next_event(&self) -> DesktopEvent {
        std::future::poll_fn(|cx| {
            let mut events = self.events.lock().expect("fake desktop lock");
            match events.queue.pop_front() {
                Some(event) => Poll::Ready(event),
                None => {
                    events.waiting = Some(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
        .await
    }
}

impl LeaseControl for FakeDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        let info = self
            .windows
            .lock()
            .expect("fake desktop lock")
            .iter()
            .find(|w| w.ident == req.window)
            .cloned()
            .ok_or(DesktopError::NoSuchWindow)?;
        req.check(&info)?;
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
        self.held
            .lock()
            .expect("fake desktop lock")
            .push((id, info.ident.clone()));
        self.note(DesktopCall::Leased(id));
        Ok(Leased {
            grant: LeaseGrant {
                id,
                window: info,
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

    /// Only a casement desktop can be asked: the leased window becomes the activated one.
    async fn focus(&self, lease: LeaseId) -> Result<(), DesktopError> {
        if self.caps.kind != BackendKind::Casement {
            return Err(DesktopError::Unsupported);
        }
        let ident = {
            let held = self.held.lock().expect("fake desktop lock");
            let (_, ident) = held
                .iter()
                .find(|(id, _)| *id == lease)
                .ok_or(DesktopError::NoSuchWindow)?;
            ident.clone()
        };
        let mut windows = self.windows.lock().expect("fake desktop lock");
        for window in windows.iter_mut() {
            match window.ident == ident {
                true => window.state.insert(ToplevelState::Activated),
                false => window.state.remove(&ToplevelState::Activated),
            };
        }
        drop(windows);
        self.note(DesktopCall::Focused(lease));
        Ok(())
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        self.note(DesktopCall::Glow(lease, glow));
        Ok(())
    }
}
