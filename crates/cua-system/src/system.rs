//! The real bundle: the seams of the running daemon.

use crate::a11y::LoggedAtspi;
use crate::backend::CaptureBackend;
use crate::casement::{CasementDesktop, Door};
use crate::connect::Links;
use crate::model::InferdCuaModel;
use a11y_atspi::AtspiSource;
use cua_desktop::{BackendKind, SeatIsolation, Stamp};
use cua_host::DaemonLinks;
use cua_host::IntentsGate;
use cua_host::SystemClock;
use cua_host::{A11yChoice, BackendChoice, CuadConfig};
use cua_run::{Clock, Parts, Rules, Seams};
use maskcap_wayland::StampClock;
use seat_ei::EiSeat;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use wayland_client::{ConnectError, Connection};

/// The daemon's seams: the desktop of casement when it is running (else none), EI for input, stock toplevel capture with
/// masking in cuad, AT-SPI for the tree, inferd for the model, `Intents1.Gate` for the router,
/// memoryd and the router's message path for records and reports, and the system clock. With no
/// compositor the desktop and capture are `None` and the daemon says so on the bus.
#[derive(Debug)]
pub struct SystemSeams;

impl Seams for SystemSeams {
    type Desktop = Option<CasementDesktop>;
    type Seat = EiSeat;
    type Capture = Option<CaptureBackend>;
    type A11y = Option<LoggedAtspi>;
    type Model = InferdCuaModel<porter_client::DbusTransport>;
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

/// Why the compositor `config` names could not be reached.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct ReachError(pub String);

/// What cuad reached of a compositor.
#[derive(Debug)]
pub enum Reached {
    /// casement: its desktop, and a second agent connection for capture.
    Casement {
        /// The windows and leases.
        desktop: CasementDesktop,
        /// For capture sessions.
        capture: Connection,
    },
    /// The compositor the environment names: capture only, no desktop backend.
    Environment(WaylandPair),
    /// No compositor could be reached; the daemon starts all the same.
    Unreached(String),
}

impl Reached {
    /// The compositor `config` asks for. `Detect` takes casement when `org.quire.Compositor1`
    /// has an owner on `bus`, else the environment's compositor, else none; `Only(Casement)`
    /// fails when casement is not there.
    pub async fn find(config: &CuadConfig, bus: &zbus::Connection) -> Result<Reached, ReachError> {
        match config.backend {
            BackendChoice::Only(BackendKind::Casement) => {
                let door = Door::find(bus).await.ok_or_else(|| {
                    ReachError("org.quire.Compositor1 has no owner on the session bus".into())
                })?;
                Self::casement(door).await
            }
            BackendChoice::Only(BackendKind::KwinStock) => Ok(Self::environment()),
            BackendChoice::Detect => match Door::find(bus).await {
                Some(door) => match Self::casement(door).await {
                    Ok(reached) => Ok(reached),
                    Err(error) => {
                        eprintln!("cuad: casement is running but not usable ({error})");
                        Ok(Self::environment())
                    }
                },
                None => Ok(Self::environment()),
            },
        }
    }

    /// Two agent connections from `door`: one for the desktop client, one for capture.
    async fn casement(door: Door) -> Result<Reached, ReachError> {
        let fail =
            |what: &str, error: &dyn std::fmt::Display| ReachError(format!("{what}: {error}"));
        let for_desktop = door
            .agent_connection()
            .await
            .map_err(|e| fail("Compositor1.Connect(agent)", &e))?;
        let for_capture = door
            .agent_connection()
            .await
            .map_err(|e| fail("Compositor1.Connect(agent)", &e))?;
        let capture = Connection::from_socket(UnixStream::from(for_capture))
            .map_err(|e| fail("the capture connection", &e))?;
        let desktop = CasementDesktop::start(door, for_desktop)
            .await
            .map_err(|e| fail("the desktop client", &e))?;
        Ok(Reached::Casement { desktop, capture })
    }

    fn environment() -> Reached {
        match WaylandPair::connect_env() {
            Ok(pair) => Reached::Environment(pair),
            Err(error) => Reached::Unreached(format!("no compositor ({error})")),
        }
    }

    /// The one line the daemon logs about what it uses.
    pub fn describe(&self) -> String {
        match self {
            Reached::Casement { .. } => "backend casement (Compositor1 agent connections)".into(),
            Reached::Environment(_) => {
                "backend kwin_stock (the environment's compositor, capture only)".into()
            }
            Reached::Unreached(why) => format!("{why}; the backend is unavailable"),
        }
    }
}

impl SystemSeams {
    /// The seams of the daemon over `links` and what was `reached` of a compositor: with none,
    /// the daemon has no desktop and refuses every capture. `clock` is shared with capture. `rules` are
    /// the screen rules the host read.
    pub fn parts(
        config: &CuadConfig,
        links: Links,
        reached: Reached,
        clock: SystemClock,
        rules: Rules,
    ) -> Parts<SystemSeams> {
        let capture_over = |kind, connection| {
            Some(CaptureBackend::over(kind, connection).with_clock(Arc::new(RunClock(clock))))
        };
        let (desktop, capture) = match reached {
            Reached::Casement { desktop, capture } => {
                (Some(desktop), capture_over(BackendKind::Casement, capture))
            }
            Reached::Environment(pair) => {
                (None, capture_over(BackendKind::KwinStock, pair.capture))
            }
            Reached::Unreached(_) => (None, None),
        };
        Parts {
            desktop,
            // One seat for everyone: casement's lease scopes the input, but the seat is the
            // person's until casement's agent seat (P4).
            seat: EiSeat::new(SeatIsolation::SharedSeat),
            capture,
            a11y: match config.a11y {
                A11yChoice::Off => None,
                A11yChoice::Atspi => Some(LoggedAtspi(AtspiSource::new())),
            },
            model: links.model,
            gate: links.gate,
            events: links.events,
            clock,
            rules,
        }
    }
}
