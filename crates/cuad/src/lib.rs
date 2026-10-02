//! cuad: the computer-use daemon, as a library and a skeleton binary.
//!
//! It owns the step loop of every run: the pure machine ([`cua_run::Run`]) says what to do, the
//! [`Supervisor`] does it against the seams ([`SystemSeams`] in the daemon, `cua-fake` in tests)
//! and feeds the answers back. It serves `org.quire.Cua1` (`cua-dbus`) and
//! `org.quire.IntentProvider1` as `org.quire.Cua` with one action, `cua.run.start` ([`CuaProvider`]).
//!
//! Backends are closed sets: [`SeatBackend`] (`Ei`), [`CaptureBackend`] (`StockToplevel`,
//! `QuireMasked`), and [`cua_desktop::BackendKind`] (`CosmicFork`, `CosmicStock`, `KwinStock`).
//! On stock cosmic-comp masking is cuad's (`MaskReport::applied_by = Cuad`) and the consent
//! guard is cuad stopping its own input; the fork moves both into the compositor.
//!
//! A run's progress and result go to the requester as `prov::Message` reports through the
//! router. A message that arrives carries no authority.
//!
//! The behaviour that is built is the tables: [`guard`], [`CallerNames::classify`],
//! [`manifest_for`], [`record_of`], [`capture_kind`], [`CuadConfig::from_toml`]. The loop, the
//! links and the bus are frozen signatures with `todo!()` bodies.

mod backend;
mod callers;
mod clock;
mod config;
mod gate;
mod guard;
mod links;
mod model;
mod provider;
mod serve;
mod supervisor;
mod system;

pub use backend::{CaptureBackend, CaptureKind, CaptureSessionKind, SeatBackend, capture_kind};
pub use callers::CallerNames;
pub use clock::SystemClock;
pub use config::{BackendChoice, ConfigError, CuadConfig};
pub use gate::{IntentsCheck, IntentsGate};
pub use guard::guard;
pub use links::{DaemonLinks, RecordContext, RecordError, record_of};
pub use model::InferdCuaModel;
pub use provider::{CuaProvider, ProviderError, manifest_for};
pub use serve::{ServeError, serve};
pub use supervisor::{Supervisor, mint_run_id};
pub use system::SystemSeams;
