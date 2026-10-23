//! One wayland connection driven from async code.
//!
//! A [`Pump`] owns the connection's one event queue and the state its handlers fill. Callers
//! take turns at it: [`Pump::drive`] dispatches what has arrived, asks the caller's step whether
//! it has what it was waiting for, and otherwise sleeps until the socket is readable or another
//! caller's turn dispatched something. Only the holder of the lock ever reads the socket, so no
//! caller can swallow the data another one is waiting for; `Notify` tells the others to look
//! again.
//!
//! The pump never blocks a thread: the socket is read with the non-blocking guard, and a task
//! that cannot go on waits on the runtime's reactor.

use std::fmt;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use tokio::io::unix::AsyncFd;
use tokio::sync::{Mutex, Notify, OnceCell};
use wayland_client::backend::WaylandError;
use wayland_client::{Connection, DispatchError, EventQueue};

/// Why the connection stopped being usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("wayland connection: {0}")]
pub struct PumpError(pub String);

impl From<DispatchError> for PumpError {
    fn from(error: DispatchError) -> Self {
        Self(error.to_string())
    }
}

impl From<WaylandError> for PumpError {
    fn from(error: WaylandError) -> Self {
        Self(error.to_string())
    }
}

impl From<std::io::Error> for PumpError {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

/// A duplicate of the connection's descriptor, registered with the reactor.
struct Socket(OwnedFd);

impl AsRawFd for Socket {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}

/// Flushes the requests; a full socket is not a failure, the next turn flushes again.
fn flush(connection: &Connection) -> Result<(), PumpError> {
    match connection.flush() {
        Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(()),
        other => Ok(other?),
    }
}

struct Inner<S> {
    queue: EventQueue<S>,
    state: S,
}

/// The connection, its queue and its state, shared by every caller that drives it.
pub struct Pump<S> {
    connection: Connection,
    inner: Mutex<Inner<S>>,
    woke: Notify,
    socket: OnceCell<AsyncFd<Socket>>,
}

impl<S> fmt::Debug for Pump<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pump").finish_non_exhaustive()
    }
}

impl<S: Send + 'static> Pump<S> {
    /// A pump over `queue` of `connection`, with `state` for its handlers.
    pub fn new(connection: Connection, queue: EventQueue<S>, state: S) -> Self {
        Self {
            connection,
            inner: Mutex::new(Inner { queue, state }),
            woke: Notify::new(),
            socket: OnceCell::new(),
        }
    }

    /// The connection.
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    // tokio 1.54 deprecates `AsyncFd::new` for types whose `as_raw_fd` may not stay the same open
    // descriptor (RUSTSEC-2026-0386); its replacement is `unsafe`. `Socket` owns its descriptor and
    // always returns it, so the registration stays valid for the `AsyncFd`'s whole life.
    #[expect(
        deprecated,
        reason = "Socket owns its fd; the replacement needs unsafe"
    )]
    async fn socket(&self) -> Result<&AsyncFd<Socket>, PumpError> {
        self.socket
            .get_or_try_init(|| async {
                let own = self.connection.backend().poll_fd().try_clone_to_owned()?;
                Ok::<_, PumpError>(AsyncFd::new(Socket(own))?)
            })
            .await
    }

    /// Runs `f` on the state now if no other caller holds the pump, and flushes; for a `Drop`,
    /// which cannot wait. `None` when the pump was busy.
    pub fn try_with<R>(&self, f: impl FnOnce(&mut S) -> R) -> Option<R> {
        let mut inner = self.inner.try_lock().ok()?;
        let out = f(&mut inner.state);
        let _ = flush(&self.connection);
        Some(out)
    }

    /// Runs `step` on the state after every dispatch, and again whenever something new arrives,
    /// until it returns a value. `step` may send requests (they are flushed before the pump
    /// waits). An error means the connection is broken or closed.
    pub async fn drive<R>(
        &self,
        mut step: impl FnMut(&mut S) -> Option<R>,
    ) -> Result<R, PumpError> {
        let socket = self.socket().await?;
        loop {
            // Registered before the state is looked at, so a dispatch by another caller between
            // the look and the wait is not missed.
            let woken = self.woke.notified();
            tokio::pin!(woken);
            woken.as_mut().enable();
            {
                let mut inner = self.inner.lock().await;
                let Inner { queue, state } = &mut *inner;
                let mut news = false;
                loop {
                    news |= queue.dispatch_pending(state)? > 0;
                    if let Some(done) = step(state) {
                        if news {
                            self.woke.notify_waiters();
                        }
                        flush(&self.connection)?;
                        return Ok(done);
                    }
                    flush(&self.connection)?;
                    let Some(guard) = queue.prepare_read() else {
                        continue;
                    };
                    match guard.read() {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(WaylandError::Io(e)) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            break;
                        }
                        Err(e) => return Err(e.into()),
                    }
                }
                if news {
                    self.woke.notify_waiters();
                }
            }
            tokio::select! {
                ready = socket.readable() => ready?.clear_ready(),
                () = &mut woken => {}
            }
        }
    }
}
