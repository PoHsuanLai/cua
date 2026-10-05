//! The real bundle: the seams of the running daemon.

use crate::backend::{CaptureBackend, SeatBackend, capture_kind};
use crate::clock::SystemClock;
use crate::config::{A11yChoice, BackendChoice, CuadConfig};
use crate::connect::Links;
use crate::desktop::{SystemA11y, SystemDesktop};
use crate::ei_socket::CosmicEiSockets;
use crate::gate::IntentsGate;
use crate::links::DaemonLinks;
use crate::model::InferdCuaModel;
use a11y_tree::AtspiSource;
use cua_desktop::{BackendKind, CosmicDesktop, SeatIsolation, Stamp};
use cua_run::{Clock, Parts, Seams};
use maskcap::StampClock;
use seat_input::EiSeat;
use std::sync::Arc;
use wayland_client::{ConnectError, Connection};

/// The daemon's seams: COSMIC stock for the desktop, EI for input, stock toplevel capture with
/// masking in cuad, AT-SPI for the tree, inferd for the model, `Intents1.Gate` for the router,
/// memoryd and the router's message path for records and reports, and the system clock. With no
/// compositor the desktop and capture are the unavailable cases of their enums and the daemon
/// says so on the bus.
#[derive(Debug)]
pub struct SystemSeams;

impl Seams for SystemSeams {
    type Desktop = SystemDesktop;
    type Seat = SeatBackend;
    type Capture = CaptureBackend;
    type A11y = SystemA11y;
    type Model = InferdCuaModel<porter_client::AnyTransport>;
    type Gate = IntentsGate<docket_client::DbusTransport>;
    type Events = DaemonLinks<almanac_client::DbusTransport, docket_client::DbusTransport>;
    type Time = SystemClock;
}

/// Two connections to the compositor: the desktop's and capture's. They do not share one,
/// because each is driven by its own pump, which owns the connection's event queue.
#[derive(Debug)]
pub struct WaylandPair {
    /// For toplevels and leases.
    pub desktop: Connection,
    /// For capture sessions.
    pub capture: Connection,
}

impl WaylandPair {
    /// Connects twice to the compositor the environment names (`WAYLAND_DISPLAY`,
    /// `XDG_RUNTIME_DIR`).
    pub fn connect_env() -> Result<Self, ConnectError> {
        Ok(Self {
            desktop: Connection::connect_to_env()?,
            capture: Connection::connect_to_env()?,
        })
    }
}

/// Frames are stamped on the clock the run machine reads.
#[derive(Debug)]
struct RunClock(SystemClock);

impl StampClock for RunClock {
    fn now(&self) -> Stamp {
        self.0.now()
    }
}

impl SystemSeams {
    /// The backend `config` asks for with a compositor to reach: `Detect` is stock COSMIC.
    fn kind_of(config: &CuadConfig) -> BackendKind {
        match config.backend {
            BackendChoice::Detect => BackendKind::CosmicStock,
            BackendChoice::Only(kind) => kind,
        }
    }

    /// The seams of the daemon over `links`. `wayland` is `None` when no compositor could be
    /// reached: the daemon then has no desktop and refuses every capture. `sockets` is where
    /// stock's EIS socket comes from; `clock` is shared with capture.
    pub fn parts(
        config: &CuadConfig,
        links: Links,
        wayland: Option<WaylandPair>,
        sockets: CosmicEiSockets,
        clock: SystemClock,
    ) -> Parts<SystemSeams> {
        let kind = Self::kind_of(config);
        let (desktop, capture) = match wayland {
            Some(pair) => (
                SystemDesktop::Cosmic(Box::new(
                    CosmicDesktop::over(pair.desktop, kind).with_sender_socket(Arc::new(sockets)),
                )),
                CaptureBackend::open_kind(capture_kind(kind), pair.capture)
                    .with_clock(Arc::new(RunClock(clock))),
            ),
            None => (SystemDesktop::Absent, CaptureBackend::Unavailable),
        };
        Parts {
            desktop,
            // Stock has one seat for everyone; the fork's agent seat is the casement backend's.
            seat: SeatBackend::Ei(EiSeat::new(SeatIsolation::SharedSeat)),
            capture,
            a11y: match config.a11y {
                A11yChoice::Off => SystemA11y::Unavailable,
                A11yChoice::Atspi => SystemA11y::Atspi(AtspiSource::new()),
            },
            model: links.model,
            gate: links.gate,
            events: links.events,
            clock,
        }
    }
}
