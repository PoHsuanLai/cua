//! One live EIS connection: the handshake, the devices the seat uses and the frames it sends.

use cua_action::{Button, ScrollDir};
use cua_desktop::{EiOrigin, GlobalRect, LeaseGrant};
use futures_util::{FutureExt, StreamExt};
use reis::ei::{self, button::ButtonState, keyboard::KeyState};
use reis::event::{Connection, Device, DeviceCapability, EiEvent};
use reis::tokio::EiConvertEventStream;
use seat_input::Keymap;
use seat_input::region::{GlobalPoint, RegionMap};
use seat_input::{Emulation, SeatError};
use seat_input::{InputOp, Press};
use std::os::fd::OwnedFd;
use std::os::unix::fs::FileExt;
use std::os::unix::net::UnixStream;
use std::time::{Duration, Instant};

/// A detent in the protocol's units (120ths of a detent).
const DETENT: i32 = 120;
/// Largest keymap read from the server.
const KEYMAP_LIMIT: u32 = 4 << 20;

/// The devices the seat drives.
#[derive(Debug)]
struct Hands {
    pointer: Device,
    keyboard: Device,
    button: Device,
    scroll: Device,
}

impl Hands {
    /// Each distinct device once.
    fn distinct(&self) -> Vec<&Device> {
        let mut out: Vec<&Device> = Vec::new();
        for d in [&self.pointer, &self.keyboard, &self.button, &self.scroll] {
            if !out.contains(&d) {
                out.push(d);
            }
        }
        out
    }
}

/// An attached seat's connection.
pub(super) struct Conn {
    map: RegionMap,
    link: Connection,
    events: EiConvertEventStream,
    hands: Hands,
    emulation: Emulation,
    sequence: u32,
    started: Instant,
}

impl std::fmt::Debug for Conn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Conn")
            .field("emulation", &self.emulation)
            .finish_non_exhaustive()
    }
}

fn protocol(e: impl std::fmt::Display) -> SeatError {
    SeatError::Protocol(e.to_string())
}

fn pick(
    devices: &[Device],
    cap: DeviceCapability,
    ok: impl Fn(&Device) -> bool,
) -> Option<&Device> {
    devices.iter().find(|d| d.has_capability(cap) && ok(d))
}

fn hands_of(devices: &[Device]) -> Option<Hands> {
    let pointer = pick(devices, DeviceCapability::PointerAbsolute, |d| {
        !d.regions().is_empty()
    })?;
    let keyboard = pick(devices, DeviceCapability::Keyboard, |d| {
        d.keymap().is_some()
    })?;
    let on_pointer = |cap| {
        pick(devices, cap, |d| d == pointer)
            .or_else(|| pick(devices, cap, |_| true))
            .cloned()
    };
    Some(Hands {
        pointer: pointer.clone(),
        keyboard: keyboard.clone(),
        button: on_pointer(DeviceCapability::Button)?,
        scroll: on_pointer(DeviceCapability::Scroll)?,
    })
}

fn keymap_of(device: &Device) -> Result<Keymap, SeatError> {
    let map = device
        .keymap()
        .ok_or_else(|| SeatError::Keymap("no keymap".into()))?;
    if map.size > KEYMAP_LIMIT {
        return Err(SeatError::Keymap(format!("{} bytes", map.size)));
    }
    let file = std::fs::File::from(
        map.fd
            .try_clone()
            .map_err(|e| SeatError::Keymap(e.to_string()))?,
    );
    let mut buf = vec![0u8; map.size as usize];
    file.read_exact_at(&mut buf, 0)
        .map_err(|e| SeatError::Keymap(e.to_string()))?;
    let end = buf.iter().position(|b| *b == 0).unwrap_or(buf.len());
    let text = std::str::from_utf8(&buf[..end]).map_err(|e| SeatError::Keymap(e.to_string()))?;
    Keymap::parse_xkb(text).map_err(|e| SeatError::Keymap(e.to_string()))
}

impl Conn {
    /// Handshakes as a sender, binds the seat, waits until the devices are resumed, reads the
    /// keymap and starts emulating.
    pub(super) async fn open(
        lease: &LeaseGrant,
        socket: OwnedFd,
    ) -> Result<(Conn, Keymap), SeatError> {
        let context = ei::Context::new(UnixStream::from(socket)).map_err(protocol)?;
        let (link, mut events) = context
            .handshake_tokio("cua-seat", ei::handshake::ContextType::Sender)
            .await
            .map_err(protocol)?;
        let mut devices: Vec<Device> = Vec::new();
        let mut resumed: Vec<Device> = Vec::new();
        let hands = loop {
            let event = events
                .next()
                .await
                .ok_or(SeatError::NotConnected)?
                .map_err(protocol)?;
            match event {
                EiEvent::SeatAdded(added) => {
                    added.seat.bind_capabilities(
                        DeviceCapability::PointerAbsolute
                            | DeviceCapability::Button
                            | DeviceCapability::Scroll
                            | DeviceCapability::Keyboard,
                    );
                    let _ = link.flush();
                }
                EiEvent::DeviceAdded(added) => devices.push(added.device),
                EiEvent::DeviceResumed(r) => resumed.push(r.device),
                EiEvent::Disconnected(_) => return Err(SeatError::Refused),
                _ => {}
            }
            let live: Vec<Device> = devices
                .iter()
                .filter(|d| resumed.contains(d))
                .cloned()
                .collect();
            if let Some(hands) = hands_of(&live) {
                break hands;
            }
        };
        let keymap = keymap_of(&hands.keyboard)?;
        let map = match lease.ei {
            EiOrigin::LeaseSocket => RegionMap::Identity,
            EiOrigin::SenderSocket => RegionMap::Offset(lease.window.geometry),
        };
        let mut conn = Conn {
            map,
            link,
            events,
            hands,
            emulation: Emulation::Stopped,
            sequence: 0,
            started: Instant::now(),
        };
        conn.set_emulation(Emulation::Running)?;
        Ok((conn, keymap))
    }

    pub(super) fn emulation(&self) -> Emulation {
        self.emulation
    }

    /// Milliseconds since the connection opened.
    pub(super) fn age_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn micros(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// Starts or stops emulating on every device in use; a no-op when already there.
    pub(super) fn set_emulation(&mut self, state: Emulation) -> Result<(), SeatError> {
        if self.emulation == state {
            return Ok(());
        }
        let serial = self.link.serial();
        for device in self.hands.distinct() {
            match state {
                Emulation::Running => {
                    device.device().start_emulating(serial, self.sequence);
                    self.sequence = self.sequence.wrapping_add(1);
                }
                Emulation::Stopped => device.device().stop_emulating(serial),
            }
        }
        self.emulation = state;
        self.link.flush().map_err(protocol)
    }

    /// The window moved: points map to its new frame from now on.
    pub(super) fn rebind(&mut self, frame: GlobalRect) {
        self.map = self.map.moved_to(frame);
    }

    /// Stops emulating and says goodbye.
    pub(super) fn close(mut self) {
        let _ = self.set_emulation(Emulation::Stopped);
        self.link.connection().disconnect();
        let _ = self.link.flush();
    }

    /// Takes in what the server sent meanwhile, without waiting.
    pub(super) fn drain(&mut self) -> Result<(), SeatError> {
        while let Some(next) = self.events.next().now_or_never() {
            match next {
                None | Some(Ok(EiEvent::Disconnected(_))) => return Err(SeatError::NotConnected),
                Some(Err(e)) => return Err(protocol(e)),
                Some(Ok(EiEvent::DevicePaused(p)))
                    if self.hands.distinct().contains(&&p.device) =>
                {
                    return Err(SeatError::Refused);
                }
                Some(Ok(_)) => {}
            }
        }
        Ok(())
    }

    fn frame(&self, device: &Device) {
        device.device().frame(self.link.serial(), self.micros());
    }

    /// Sends one operation and its frame.
    pub(super) async fn send(&self, op: &InputOp) -> Result<(), SeatError> {
        match op {
            InputOp::MoveTo(p) => {
                let GlobalPoint(x, y) = self.map.region_of(*p);
                // The pointer stays inside the window, not merely inside the output.
                if !self.map.contains(GlobalPoint(x, y)) {
                    return Err(SeatError::OutsideLease);
                }
                let inside = self.hands.pointer.regions().iter().any(|r| {
                    let (x, y) = (i64::from(x), i64::from(y));
                    x >= i64::from(r.x)
                        && y >= i64::from(r.y)
                        && x < i64::from(r.x) + i64::from(r.width)
                        && y < i64::from(r.y) + i64::from(r.height)
                });
                if !inside {
                    return Err(SeatError::OutsideLease);
                }
                let abs = self.interface::<ei::PointerAbsolute>(&self.hands.pointer)?;
                abs.motion_absolute(x as f32, y as f32);
                self.frame(&self.hands.pointer);
            }
            InputOp::Button { button, dir } => {
                let code = match button {
                    Button::Left => 0x110,
                    Button::Right => 0x111,
                    Button::Middle => 0x112,
                };
                let state = match dir {
                    Press::Down => ButtonState::Press,
                    Press::Up => ButtonState::Released,
                };
                self.interface::<ei::Button>(&self.hands.button)?
                    .button(code, state);
                self.frame(&self.hands.button);
            }
            InputOp::Key { key, dir } => {
                let state = match dir {
                    Press::Down => KeyState::Press,
                    Press::Up => KeyState::Released,
                };
                self.interface::<ei::Keyboard>(&self.hands.keyboard)?
                    .key(key.0, state);
                self.frame(&self.hands.keyboard);
            }
            InputOp::Scroll { dir, notches } => {
                let n = i32::from(notches.0) * DETENT;
                let (x, y) = match dir {
                    ScrollDir::Up => (0, -n),
                    ScrollDir::Down => (0, n),
                    ScrollDir::Left => (-n, 0),
                    ScrollDir::Right => (n, 0),
                };
                self.interface::<ei::Scroll>(&self.hands.scroll)?
                    .scroll_discrete(x, y);
                self.frame(&self.hands.scroll);
            }
            // No Unicode text path over EI is offered (S10).
            InputOp::Text(_) => return Err(SeatError::Refused),
            InputOp::Gap(ms) => {
                self.link.flush().map_err(protocol)?;
                tokio::time::sleep(Duration::from_millis(u64::from(ms.0))).await;
            }
        }
        Ok(())
    }

    pub(super) fn flush(&self) -> Result<(), SeatError> {
        self.link.flush().map_err(protocol)
    }

    fn interface<T: ei::Interface>(&self, device: &Device) -> Result<T, SeatError> {
        device
            .interface::<T>()
            .ok_or_else(|| SeatError::Protocol(format!("device lost {}", T::NAME)))
    }
}
