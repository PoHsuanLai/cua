//! An EIS server in the test process, on a socketpair: the other end of an `EiSeat` under test.

use crate::eis_keymap::US_KEYMAP;
use cua_desktop::EiSocket;
use reis::PendingRequestResult;
use reis::eis::{self, device::DeviceType, keyboard::KeymapType};
use reis::handshake::EisHandshaker;
use reis::request::{DeviceCapability, EisRequest, EisRequestConverter};
use seat_input::Press;
use std::io::{ErrorKind, Write};
use std::os::fd::{AsFd, OwnedFd};
use std::os::unix::net::UnixStream;
use tokio::io::{Interest, unix::AsyncFd};

/// The one output the fake advertises, as measured on stock cosmic-comp: global logical
/// coordinates, scale 1.
const OUTPUT: (u32, u32, u32, u32) = (0, 0, 1280, 800);
/// The server's first serial.
const FIRST_SERIAL: u32 = 1;

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
    server: eis::Context,
}

impl FakeEis {
    /// A server and the client socket that talks to it.
    pub fn pair() -> std::io::Result<(FakeEis, EiSocket)> {
        let (server, client) = UnixStream::pair()?;
        Ok((
            FakeEis {
                server: eis::Context::new(server)?,
            },
            EiSocket(OwnedFd::from(client)),
        ))
    }

    /// Serves the handshake, offers a keyboard with a US keymap, a pointer and a scroll device
    /// with the regions measured on stock, and returns what the sender sent until it
    /// disconnects (or drops the socket).
    pub async fn serve(&mut self) -> Vec<EisEvent> {
        let mut session = Session::new(&self.server);
        let Ok(ready) = AsyncFd::with_interest(self.server.as_fd(), Interest::READABLE) else {
            return Vec::new();
        };
        loop {
            while let Some(pending) = self.server.pending_request() {
                if session.on_pending(&self.server, pending) == Flow::Stop {
                    return session.log;
                }
            }
            let _ = self.server.flush();
            let Ok(mut guard) = ready.readable().await else {
                break;
            };
            match self.server.read() {
                Ok(_) => guard.clear_ready(),
                Err(e) if e.kind() == ErrorKind::WouldBlock => guard.clear_ready(),
                Err(_) => break,
            }
        }
        session.log
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Go,
    Stop,
}

enum Stage {
    Handshake(EisHandshaker),
    Serving(EisRequestConverter),
}

struct Session {
    stage: Stage,
    log: Vec<EisEvent>,
}

fn press(state: eis::button::ButtonState) -> Press {
    match state {
        eis::button::ButtonState::Press => Press::Down,
        _ => Press::Up,
    }
}

fn key_press(state: eis::keyboard::KeyState) -> Press {
    match state {
        eis::keyboard::KeyState::Press => Press::Down,
        _ => Press::Up,
    }
}

impl Session {
    fn new(context: &eis::Context) -> Self {
        Session {
            stage: Stage::Handshake(EisHandshaker::new(context, FIRST_SERIAL)),
            log: Vec::new(),
        }
    }

    fn on_pending(
        &mut self,
        context: &eis::Context,
        pending: PendingRequestResult<eis::Request>,
    ) -> Flow {
        let PendingRequestResult::Request(request) = pending else {
            return Flow::Stop;
        };
        let Session { stage, log } = self;
        match stage {
            Stage::Handshake(handshaker) => match handshaker.handle_request(request) {
                Ok(None) => Flow::Go,
                Ok(Some(resp)) => {
                    let converter = EisRequestConverter::new(context, resp, FIRST_SERIAL);
                    advertise_seat(converter.handle());
                    log.push(EisEvent::Handshake);
                    *stage = Stage::Serving(converter);
                    Flow::Go
                }
                Err(_) => Flow::Stop,
            },
            Stage::Serving(converter) => {
                if converter.handle_request(request).is_err() {
                    return Flow::Stop;
                }
                while let Some(request) = converter.next_request() {
                    if log_request(log, request) == Flow::Stop {
                        return Flow::Stop;
                    }
                }
                Flow::Go
            }
        }
    }
}

fn log_request(log: &mut Vec<EisEvent>, request: EisRequest) -> Flow {
    let event = match request {
        EisRequest::Disconnect => return Flow::Stop,
        EisRequest::Bind(bind) => {
            add_devices(&bind.seat, bind.capabilities);
            EisEvent::Bind
        }
        EisRequest::DeviceStartEmulating(_) => EisEvent::StartEmulating,
        EisRequest::DeviceStopEmulating(_) => EisEvent::StopEmulating,
        EisRequest::PointerMotionAbsolute(m) => EisEvent::PointerAbsolute {
            x: m.dx_absolute.round() as i32,
            y: m.dy_absolute.round() as i32,
        },
        EisRequest::Button(b) => EisEvent::Button {
            code: b.button,
            dir: press(b.state),
        },
        EisRequest::KeyboardKey(k) => EisEvent::Key {
            code: k.key,
            dir: key_press(k.state),
        },
        EisRequest::ScrollDiscrete(s) => EisEvent::ScrollDiscrete {
            x: s.discrete_dx,
            y: s.discrete_dy,
        },
        _ => return Flow::Go,
    };
    log.push(event);
    Flow::Go
}

fn advertise_seat(connection: &reis::request::Connection) {
    let _ = connection.add_seat(
        Some("default"),
        DeviceCapability::PointerAbsolute
            | DeviceCapability::Keyboard
            | DeviceCapability::Scroll
            | DeviceCapability::Button,
    );
    let _ = connection.flush();
}

/// A keymap file the keyboard device can send.
fn keymap_file() -> Option<(OwnedFd, u32)> {
    let fd = rustix::fs::memfd_create("cua-fake-keymap", rustix::fs::MemfdFlags::CLOEXEC).ok()?;
    let mut file = std::fs::File::from(fd);
    let mut text = US_KEYMAP.as_bytes().to_vec();
    text.push(0);
    file.write_all(&text).ok()?;
    let size = u32::try_from(text.len()).ok()?;
    Some((OwnedFd::from(file), size))
}

fn add_devices(seat: &reis::request::Seat, wanted: impl IntoIterator<Item = DeviceCapability>) {
    let wanted: Vec<DeviceCapability> = wanted.into_iter().collect();
    let has = |c| wanted.contains(&c);
    if has(DeviceCapability::Keyboard) {
        let keymap = keymap_file();
        let device = seat.add_device(
            Some("virtual keyboard"),
            DeviceType::Virtual,
            DeviceCapability::Keyboard.into(),
            |d| {
                if let (Some(keyboard), Some((fd, size))) =
                    (d.interface::<eis::Keyboard>(), keymap.as_ref())
                {
                    keyboard.keymap(KeymapType::Xkb, *size, fd.as_fd());
                }
            },
        );
        device.resumed();
    }
    if has(DeviceCapability::PointerAbsolute) {
        let device = seat.add_device(
            Some("virtual absolute pointer"),
            DeviceType::Virtual,
            DeviceCapability::PointerAbsolute | DeviceCapability::Button | DeviceCapability::Scroll,
            |d| {
                let (x, y, w, h) = OUTPUT;
                d.device().region(x, y, w, h, 1.0);
            },
        );
        device.resumed();
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
