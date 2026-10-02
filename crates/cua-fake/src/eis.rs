//! An EIS server in the test process, on a socketpair: the other end of an `EiSeat` under test.

use cua_desktop::EiSocket;
use seat_input::Press;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;

/// What the fake server saw from a sender, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EisEvent {
    /// The handshake completed.
    Handshake,
    /// The sender bound a device capability.
    Bind,
    /// `start_emulating`.
    StartEmulating,
    /// An absolute pointer position, in the region's coordinates.
    PointerAbsolute {
        /// X.
        x: i32,
        /// Y.
        y: i32,
    },
    /// A button.
    Button {
        /// The evdev code.
        code: u32,
        /// Pressed or released.
        dir: Press,
    },
    /// A key.
    Key {
        /// The evdev code.
        code: u32,
        /// Pressed or released.
        dir: Press,
    },
    /// Discrete scroll, in 120ths of a detent.
    ScrollDiscrete {
        /// Horizontal.
        x: i32,
        /// Vertical.
        y: i32,
    },
    /// `stop_emulating`.
    StopEmulating,
}

/// The server end: a reis `eis` context over one end of a socketpair.
#[derive(Debug)]
pub struct FakeEis {
    server: reis::eis::Context,
}

impl FakeEis {
    /// A server and the client socket that talks to it.
    pub fn pair() -> std::io::Result<(FakeEis, EiSocket)> {
        let (server, client) = UnixStream::pair()?;
        Ok((
            FakeEis {
                server: reis::eis::Context::new(server)?,
            },
            EiSocket(OwnedFd::from(client)),
        ))
    }

    /// Serves the handshake, offers a keyboard with a US keymap, a pointer and a scroll device
    /// with the regions measured on stock, and returns what the sender sent until it
    /// disconnects.
    pub async fn serve(&mut self) -> Vec<EisEvent> {
        let _ = &self.server;
        todo!("the reis eis handshake, devices and the event log (ei_seat_against_fake_eis)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pair_gives_a_server_and_a_connected_client_socket() {
        let (_server, client) = FakeEis::pair().expect("socketpair");
        let fd = &client.0;
        assert!(std::os::fd::AsRawFd::as_raw_fd(fd) >= 0);
    }
}
