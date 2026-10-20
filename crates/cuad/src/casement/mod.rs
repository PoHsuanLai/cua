//! The desktop on casement, our own compositor: windows from the agent connection's desktop
//! client, and a lease per window from `Compositor1.LeaseWindow`.
//!
//! This is the interim backend. The compositor scopes the leased input to one window and its
//! popups and refuses it anywhere else, but it is still the person's seat and cursor
//! (`SeatIsolation::SharedSeat`) and cuad masks the frames. The agent seat of its own, the
//! compositor's masking and the agent Space are casement P4.

mod leases;
mod map;
mod proxy;
mod watch;

pub use proxy::Door;

use cua_desktop::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, EiOrigin, EiSocket, GlowState,
    LeaseControl, LeaseGrant, LeaseId, LeaseRequest, Leased, RunMode, SeatIsolation, Suspend,
    ToplevelInfo,
};
use leases::Leases;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::{Mutex as AsyncMutex, mpsc, oneshot};
use watch::{Shared, Windows};

/// The input devices a lease asks for: keyboard (1), pointer (2) and touch (4).
const DEVICES: u32 = 1 | 2 | 4;

fn protocol(error: impl std::fmt::Display) -> DesktopError {
    DesktopError::Protocol(error.to_string())
}

/// The desktop casement reports and leases.
#[derive(Debug)]
pub struct CasementDesktop {
    door: Door,
    windows: Arc<Mutex<Windows>>,
    leases: Arc<Mutex<Leases>>,
    events: AsyncMutex<mpsc::UnboundedReceiver<DesktopEvent>>,
}

impl CasementDesktop {
    /// The desktop over `fd`, a connection with the agent role from `door`. It binds the
    /// client's globals and learns the first window list before it returns.
    pub async fn start(door: Door, fd: OwnedFd) -> Result<Self, DesktopError> {
        let windows = Arc::new(Mutex::new(Err(protocol("the window list is not read yet"))));
        let leases = Arc::new(Mutex::new(Leases::default()));
        let (events, receiver) = mpsc::unbounded_channel();
        let (ready, answer) = oneshot::channel();
        let shared = Shared {
            windows: windows.clone(),
            leases: leases.clone(),
            events,
        };
        std::thread::Builder::new()
            .name("cuad-casement".into())
            .spawn(move || watch::run(fd, &shared, ready))
            .map_err(protocol)?;
        answer
            .await
            .map_err(|_| protocol("the desktop client stopped before it was ready"))?
            .map_err(protocol)?;
        Ok(Self {
            door,
            windows,
            leases,
            events: AsyncMutex::new(receiver),
        })
    }

    fn leases(&self) -> MutexGuard<'_, Leases> {
        self.leases.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn listed(&self) -> Windows {
        self.windows
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The toplevel `req` names, checked leasable.
    fn window(&self, req: &LeaseRequest) -> Result<ToplevelInfo, DesktopError> {
        let info = self
            .listed()?
            .into_iter()
            .find(|w| w.ident == req.window)
            .ok_or(DesktopError::NoSuchWindow)?;
        req.check(&info)?;
        Ok(info)
    }
}

impl Desktop for CasementDesktop {
    fn caps(&self) -> BackendCaps {
        BackendCaps::of(BackendKind::Casement)
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        self.listed()
    }

    async fn next_event(&self) -> DesktopEvent {
        match self.events.lock().await.recv().await {
            Some(event) => event,
            // The thread ended and everything it had to say is said.
            None => std::future::pending().await,
        }
    }
}

impl LeaseControl for CasementDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        // Only in place: another Space's window is not rendered for capture, and there is no
        // agent Space or nested host to run in.
        match req.mode {
            RunMode::InPlace => {}
            mode => return Err(DesktopError::ModeUnsupported(mode)),
        }
        let window = self.window(&req)?;
        self.leases().claim(&req.window)?;
        match self
            .door
            .proxy()
            .lease_window(req.window.as_str(), DEVICES)
            .await
        {
            Ok((number, fd)) => {
                let id = LeaseId(number);
                self.leases().grant(&req.window, id);
                Ok(Leased {
                    grant: LeaseGrant {
                        id,
                        window,
                        seat: SeatIsolation::SharedSeat,
                        ei: EiOrigin::SenderSocket,
                    },
                    socket: EiSocket(fd.into()),
                })
            }
            Err(error) => {
                // Nothing was granted: the window is free again.
                self.leases().abandon(&req.window);
                Err(protocol(error))
            }
        }
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        // A lease whose window closed has ended here already, but casement still holds its socket
        // until it is given back; ending a lease casement does not know is not an error.
        let _ = self.leases().end(lease);
        self.door.proxy().end_lease(lease.0).await.map_err(protocol)
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        // On a shared seat cuad stops emulating; the compositor is told nothing.
        let _ = why;
        self.leases().check(lease)
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.leases().check(lease)
    }

    async fn focus(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.leases().check(lease)?;
        self.door
            .proxy()
            .focus_lease(lease.0)
            .await
            .map_err(protocol)
    }

    async fn glow(&self, _lease: LeaseId, _glow: GlowState) -> Result<(), DesktopError> {
        // The shell's orb is the only glow here.
        Ok(())
    }
}
