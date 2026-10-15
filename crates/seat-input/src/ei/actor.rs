//! The thread that owns one EIS connection.
//!
//! reis proxies are not `Send`, and `AgentSeat`'s futures must be, so each attached lease gets a
//! thread with its own current-thread runtime. The seat talks to it over a channel and waits on
//! one-shot replies; the thread ends when the seat drops its end or the lease detaches.

use super::conn::Conn;
use crate::keymap::Keymap;
use crate::op::{InputOp, Press};
use crate::seat::{Emulation, SeatError};
use cua_desktop::{LeaseGrant, Stamp};
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot, watch};

type Reply<T> = oneshot::Sender<Result<T, SeatError>>;

/// What the seat asks of the connection's thread.
#[derive(Debug)]
pub(super) enum Cmd {
    /// The ops, the stop generation they were queued under, and the reply.
    Perform(Vec<InputOp>, u64, Reply<Stamp>),
    Emulation(Emulation, Reply<()>),
    Detach(Reply<()>),
}

/// The seat's end of a connection thread.
#[derive(Debug)]
pub(super) struct Link {
    pub(super) lease: cua_desktop::LeaseId,
    tx: mpsc::Sender<Cmd>,
    /// The stop generation: bumped when a `perform` is abandoned, read by the thread between ops.
    stop: Arc<watch::Sender<u64>>,
}

/// Bumps the stop generation when dropped before `finish`: a `perform` whose future is dropped
/// (cancel, pause, the kill chord, a deadline) takes the queued and running ops with it.
pub(super) struct StopOnDrop {
    stop: Arc<watch::Sender<u64>>,
    state: Watching,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Watching {
    Running,
    Finished,
}

impl StopOnDrop {
    /// The call ran to its end: nothing to stop.
    pub(super) fn finish(mut self) {
        self.state = Watching::Finished;
    }
}

impl Drop for StopOnDrop {
    fn drop(&mut self) {
        if self.state == Watching::Running {
            self.stop.send_modify(|g| *g = g.wrapping_add(1));
        }
    }
}

impl Link {
    /// The generation a command queued now belongs to.
    pub(super) fn generation(&self) -> u64 {
        *self.stop.borrow()
    }

    /// A guard that stops the thread's work if dropped before `finish`.
    pub(super) fn stop_on_drop(&self) -> StopOnDrop {
        StopOnDrop {
            stop: self.stop.clone(),
            state: Watching::Running,
        }
    }

    /// Sends a command built around a reply and waits for the answer. A thread that is gone
    /// means the connection is.
    pub(super) async fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> Cmd) -> Result<T, SeatError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(make(tx))
            .await
            .map_err(|_| SeatError::NotConnected)?;
        rx.await.map_err(|_| SeatError::NotConnected)?
    }
}

/// Starts the thread, handshakes on it and waits until the seat is ready.
pub(super) async fn spawn(
    lease: &LeaseGrant,
    socket: OwnedFd,
) -> Result<(Link, Keymap), SeatError> {
    let (ready_tx, ready_rx) = oneshot::channel();
    let (tx, rx) = mpsc::channel(8);
    let (stop_tx, stop_rx) = watch::channel(0_u64);
    let grant = lease.clone();
    std::thread::Builder::new()
        .name("cua-ei".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(rt) => rt.block_on(run(grant, socket, ready_tx, rx, stop_rx)),
                Err(e) => {
                    let _ = ready_tx.send(Err(SeatError::Protocol(e.to_string())));
                }
            }
        })
        .map_err(|e| SeatError::Protocol(e.to_string()))?;
    let keymap = ready_rx.await.map_err(|_| SeatError::NotConnected)??;
    Ok((
        Link {
            lease: lease.id,
            tx,
            stop: Arc::new(stop_tx),
        },
        keymap,
    ))
}

async fn run(
    grant: LeaseGrant,
    socket: OwnedFd,
    ready: Reply<Keymap>,
    mut rx: mpsc::Receiver<Cmd>,
    mut stop: watch::Receiver<u64>,
) {
    let mut conn = match Conn::open(&grant, socket).await {
        Ok((conn, keymap)) => {
            let _ = ready.send(Ok(keymap));
            conn
        }
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };
    while let Some(cmd) = rx.recv().await {
        match cmd {
            Cmd::Perform(ops, generation, reply) => {
                let result = perform(&mut conn, &ops, generation, &mut stop).await;
                let gone = result == Err(SeatError::NotConnected);
                let _ = reply.send(result);
                if gone {
                    return;
                }
            }
            Cmd::Emulation(state, reply) => {
                let _ = reply.send(conn.set_emulation(state));
            }
            Cmd::Detach(reply) => {
                conn.close();
                let _ = reply.send(Ok(()));
                return;
            }
        }
    }
    conn.close();
}

/// Whether the work queued under `generation` is still wanted.
fn wanted(stop: &watch::Receiver<u64>, generation: u64) -> Result<(), SeatError> {
    if *stop.borrow() == generation {
        Ok(())
    } else {
        Err(SeatError::Suspended)
    }
}

async fn perform(
    conn: &mut Conn,
    ops: &[InputOp],
    generation: u64,
    stop: &mut watch::Receiver<u64>,
) -> Result<Stamp, SeatError> {
    // Marks the current generation seen, so only a later stop wakes a pause below.
    drop(stop.borrow_and_update());
    wanted(stop, generation)?;
    conn.drain()?;
    if conn.emulation() == Emulation::Stopped {
        return Err(SeatError::Suspended);
    }
    let mut held: Vec<InputOp> = Vec::new();
    for op in ops {
        let done = match wanted(stop, generation) {
            Ok(()) => step(conn, op, stop).await,
            Err(e) => Err(e),
        };
        match done {
            Ok(()) => note_held(&mut held, op),
            Err(e) => {
                release(conn, &held).await;
                return Err(e);
            }
        }
    }
    conn.flush()?;
    Ok(Stamp(conn.age_ms()))
}

/// One op; a pause between steps ends early when the work is stopped.
async fn step(conn: &Conn, op: &InputOp, stop: &mut watch::Receiver<u64>) -> Result<(), SeatError> {
    match op {
        InputOp::Gap(ms) => {
            conn.flush()?;
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(u64::from(ms.0))) => Ok(()),
                _ = stop.changed() => Err(SeatError::Suspended),
            }
        }
        other => conn.send(other).await,
    }
}

/// Tracks the keys and buttons a stopped batch would leave down.
fn note_held(held: &mut Vec<InputOp>, op: &InputOp) {
    match op {
        InputOp::Key {
            key,
            dir: Press::Down,
        } => held.push(InputOp::Key {
            key: *key,
            dir: Press::Up,
        }),
        InputOp::Button {
            button,
            dir: Press::Down,
        } => held.push(InputOp::Button {
            button: *button,
            dir: Press::Up,
        }),
        InputOp::Key {
            key,
            dir: Press::Up,
        } => held.retain(|h| !matches!(h, InputOp::Key { key: k, .. } if k == key)),
        InputOp::Button {
            button,
            dir: Press::Up,
        } => held.retain(|h| !matches!(h, InputOp::Button { button: b, .. } if b == button)),
        _ => {}
    }
}

/// Lets go of what a stopped batch left down, best effort.
async fn release(conn: &Conn, held: &[InputOp]) {
    for up in held.iter().rev() {
        let _ = conn.send(up).await;
    }
    let _ = conn.flush();
}
