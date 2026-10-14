//! The real bundle: the seams of the running daemon.

use crate::backend::{CaptureBackend, SeatBackend, capture_kind};
use crate::connect::Links;
use crate::desktop::{SystemA11y, SystemDesktop};
use crate::model::InferdCuaModel;
use a11y_tree::AtspiSource;
use cua_desktop::{BackendKind, SeatIsolation, Stamp};
use cua_host::DaemonLinks;
use cua_host::IntentsGate;
use cua_host::SystemClock;
use cua_host::{A11yChoice, BackendChoice, CuadConfig};
use cua_run::{Clock, Parts, Seams};
use maskcap::StampClock;
use seat_input::EiSeat;
use std::sync::Arc;
use wayland_client::{ConnectError, Connection};

/// The daemon's seams: no compositor backend yet for the desktop, EI for input, stock toplevel capture with
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

/// The connection to the compositor, driven by capture's pump, which owns its event queue.
#[derive(Debug)]
pub struct WaylandPair {
    /// For capture sessions.
    pub capture: Connection,
}

impl WaylandPair {
    /// Connects to the compositor the environment names (`WAYLAND_DISPLAY`, `XDG_RUNTIME_DIR`).
    pub fn connect_env() -> Result<Self, ConnectError> {
        Ok(Self {
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
    /// The backend `config` asks for with a compositor to reach: `Detect` is the stock row.
    fn kind_of(config: &CuadConfig) -> BackendKind {
        match config.backend {
            BackendChoice::Detect => BackendKind::KwinStock,
            BackendChoice::Only(kind) => kind,
        }
    }

    /// The seams of the daemon over `links`. `wayland` is `None` when no compositor could be
    /// reached: the daemon then has no desktop and refuses every capture. `clock` is shared with
    /// capture.
    pub fn parts(
        config: &CuadConfig,
        links: Links,
        wayland: Option<WaylandPair>,
        clock: SystemClock,
    ) -> Parts<SystemSeams> {
        let kind = Self::kind_of(config);
        let capture = match wayland {
            Some(pair) => CaptureBackend::open_kind(capture_kind(kind), pair.capture)
                .with_clock(Arc::new(RunClock(clock))),
            None => CaptureBackend::Unavailable,
        };
        Parts {
            desktop: SystemDesktop::Absent,
            // Stock has one seat for everyone; the agent seat is the casement backend's.
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
