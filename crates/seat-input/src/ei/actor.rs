//! The thread that owns one EIS connection.
//!
//! reis proxies are not `Send`, and `AgentSeat`'s futures must be, so each attached lease gets a
//! thread with its own current-thread runtime. The seat talks to it over a channel and waits on
//! one-shot replies; the thread ends when the seat drops its end or the lease detaches.

use super::conn::Conn;
use crate::keymap::Keymap;
use crate::op::InputOp;
use crate::seat::{Emulation, SeatError};
use cua_desktop::{LeaseGrant, Stamp};
use std::os::fd::OwnedFd;
use tokio::sync::{mpsc, oneshot};

type Reply<T> = oneshot::Sender<Result<T, SeatError>>;

/// What the seat asks of the connection's thread.
#[derive(Debug)]
pub(super) enum Cmd {
    Perform(Vec<InputOp>, Reply<Stamp>),
    Emulation(Emulation, Reply<()>),
    Detach(Reply<()>),
}

/// The seat's end of a connection thread.
#[derive(Debug)]
pub(super) struct Link {
    pub(super) lease: cua_desktop::LeaseId,
    tx: mpsc::Sender<Cmd>,
}

impl Link {
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
    let grant = lease.clone();
    std::thread::Builder::new()
        .name("cua-ei".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            match runtime {
                Ok(rt) => rt.block_on(run(grant, socket, ready_tx, rx)),
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
        },
        keymap,
    ))
}

async fn run(
    grant: LeaseGrant,
    socket: OwnedFd,
    ready: Reply<Keymap>,
    mut rx: mpsc::Receiver<Cmd>,
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
            Cmd::Perform(ops, reply) => {
                let result = perform(&mut conn, &ops).await;
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

async fn perform(conn: &mut Conn, ops: &[InputOp]) -> Result<Stamp, SeatError> {
    conn.drain()?;
    if conn.emulation() == Emulation::Stopped {
        return Err(SeatError::Suspended);
    }
    for op in ops {
        conn.send(op).await?;
    }
    conn.flush()?;
    Ok(Stamp(conn.age_ms()))
}
