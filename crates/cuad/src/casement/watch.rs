//! The desktop client on its own thread: it reads the agent connection, keeps the window list
//! current and turns what changed into [`DesktopEvent`]s.
//!
//! The client is a blocking, single-owner object, so one thread owns it and the async side
//! reads what the thread publishes. The thread ends when the connection does.

use super::leases::Leases;
use super::map::{self, Idents};
use cua_desktop::{DesktopError, DesktopEvent, RevokeReason, ToplevelInfo};
use desktop_client::client::{DesktopClient, WaylandRoute, WireError};
use desktop_client::{Desktop, DesktopConfig};
use std::error::Error;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

/// The windows as of the last dispatch, or why the desktop cannot list them.
pub(super) type Windows = Result<Vec<ToplevelInfo>, DesktopError>;

/// Nobody reads the events any more: the desktop was dropped.
#[derive(Debug)]
struct Gone;

/// What the thread shares with the desktop.
#[derive(Debug, Clone)]
pub(super) struct Shared {
    pub(super) windows: Arc<Mutex<Windows>>,
    pub(super) leases: Arc<Mutex<Leases>>,
    pub(super) events: UnboundedSender<DesktopEvent>,
}

impl Shared {
    fn publish(&self, windows: Windows) {
        *self.windows.lock().unwrap_or_else(PoisonError::into_inner) = windows;
    }

    /// Sends `event`; refused when nobody is listening any more.
    fn send(&self, event: DesktopEvent) -> Result<(), Gone> {
        self.events.send(event).map_err(|_| Gone)
    }

    /// A window closed: its lease ended with it.
    fn closed(&self, event: &DesktopEvent) {
        let DesktopEvent::WindowClosed(window) = event else {
            return;
        };
        let lease = self
            .leases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed(window);
        if let Some(lease) = lease {
            let _ = self.send(DesktopEvent::LeaseRevoked {
                lease,
                why: RevokeReason::WindowClosed,
            });
        }
    }

    /// The connection is gone: say so for the list, and end every lease on it.
    fn lost(&self, error: &WireError) {
        self.publish(Err(DesktopError::Protocol(format!(
            "the compositor connection ended: {error}"
        ))));
        let ended = self
            .leases
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain();
        for lease in ended {
            let _ = self.send(DesktopEvent::LeaseRevoked {
                lease,
                why: RevokeReason::Compositor,
            });
        }
    }
}

/// The error and what caused it, on one line.
pub(super) fn chain(error: &WireError) -> String {
    let mut text = error.to_string();
    let mut cause = error.source();
    while let Some(inner) = cause {
        text = format!("{text}: {inner}");
        cause = inner.source();
    }
    text
}

/// Binds the client on the handed connection and tells `ready` how that went, then runs until
/// the connection ends or nobody listens: dispatch, publish, send.
pub(super) fn run(fd: OwnedFd, shared: &Shared, ready: oneshot::Sender<Result<(), WireError>>) {
    let route = WaylandRoute::Handed(fd);
    let mut client = match DesktopClient::connect(route, DesktopConfig::default()) {
        Ok(client) => client,
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut known: Idents = map::idents(client.state());
    shared.publish(map::infos(client.state()));
    let _ = ready.send(Ok(()));
    loop {
        if let Err(error) = client.blocking_dispatch() {
            shared.lost(&error);
            return;
        }
        let raised = client.drain_events();
        let now = map::idents(client.state());
        for event in map::events(&raised, &known, &now) {
            let sent = shared.send(event.clone());
            shared.closed(&event);
            if sent.is_err() {
                return;
            }
        }
        known = now;
        shared.publish(map::infos(client.state()));
    }
}
