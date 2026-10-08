//! cuad: the computer-use daemon, as a library and a binary.
//!
//! It owns the step loop of every run: the pure machine ([`cua_run::Run`]) says what to do, the
//! [`Supervisor`] does it against the seams ([`SystemSeams`] in the daemon, `cua-fake` in tests)
//! and feeds the answers back. It serves `org.quire.Cua1` (`cua-dbus`) and
//! `org.quire.IntentProvider1` as `org.quire.Cua` with one action, `cua.run.start` ([`CuaProvider`]).
//!
//! Backends are closed sets: [`SeatBackend`] (`Ei`), [`CaptureBackend`] (`StockToplevel`,
//! `QuireMasked`), and [`cua_desktop::BackendKind`] (`CosmicStock`, `KwinStock`).
//! On stock cosmic-comp masking is cuad's (`MaskReport::applied_by = Cuad`) and the consent
//! guard is cuad stopping its own input; the fork moves both into the compositor.
//!
//! A run's progress and result go to the requester as `prov::Message` reports through the
//! router. A message that arrives carries no authority.
//!
//! The behaviour that is built: the tables ([`guard`], [`CallerNames::classify`],
//! [`manifest_for`], [`record_of`], [`capture_kind`], [`CuadConfig::from_toml`]), the supervisor
//! ([`Supervisor`]: a task per run over [`cua_run::Exec`], with the person's controls, the
//! desktop's events and the clock racing whatever the run waits on), the links ([`IntentsGate`],
//! [`DaemonLinks`], [`InferdCuaModel`], [`Links`]) and the bus ([`serve`], [`serve_on`]). The
//! backends behind [`SeatBackend`] and [`CaptureBackend`] are filled by their own crates, and the
//! binary builds [`SystemSeams`] from them: with no compositor it still starts and reports the
//! backend as unavailable (`Backend = none`).

mod backend;
mod bus;
mod callers;
mod clock;
mod config;
mod connect;
mod desktop;
mod drive;
mod ei_socket;
mod events;
mod frame_fd;
mod gate;
mod guard;
mod links;
mod lock;
mod model;
mod provider;
mod serve;
mod settings;
mod supervisor;
mod system;

pub use backend::{CaptureBackend, CaptureKind, CaptureSessionKind, SeatBackend, capture_kind};
pub use callers::CallerNames;
pub use clock::SystemClock;
pub use config::{A11yChoice, BackendChoice, ConfigError, CuadConfig};
pub use connect::Links;
pub use desktop::{SystemA11y, SystemDesktop};
pub use drive::Drivable;
pub use ei_socket::{CosmicEiSockets, EI_BUS_NAME, EI_INTERFACE, EI_PATH, OSK_NAME};
pub use events::{RunView, SupervisorEvent};
pub use frame_fd::{FrameFdError, frame_fd};
pub use gate::{IntentsCheck, IntentsGate};
pub use guard::guard;
pub use links::{DaemonLinks, RecordContext, RecordError, record_of};
pub use model::InferdCuaModel;
pub use provider::{CuaProvider, ModeChoice, ProviderError, StartRequest, manifest_for};
pub use serve::{ServeError, Serving, serve, serve_on};
pub use settings::{
    AppAllow, AppRule, Apps, CuaSettings, DEBOUNCE, Fallback, Loaded, Locator, SCHEMA,
    SETTINGS_FILE, SettingsWatch, WatchState, Why, is_browser, read,
};
pub use supervisor::{Supervisor, mint_run_id};
pub use system::{SystemSeams, WaylandPair};
