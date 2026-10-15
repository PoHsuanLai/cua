//! cua-host: the run executor as a library.
//!
//! The pure machine ([`cua_run::Run`]) says what to do; the [`Supervisor`] does it against the
//! seams ([`cua_run::Seams`]) and feeds the answers back: a task per run over [`cua_run::Exec`],
//! with the person's controls, the desktop's events and the clock racing whatever the run waits
//! on. An app that wants to run a computer-use task in its own process builds the seams (the
//! backends are its own, `cua-fake` in tests) and a [`Supervisor`] over them.
//!
//! Nothing here opens a connection or starts a backend: this crate links no wayland, EIS,
//! accessibility or bus crate, and no file watcher. What is here:
//!
//! - the supervisor, its drive loop ([`Drivable`]) and its event stream ([`SupervisorEvent`]);
//! - the safety tables the supervisor applies to every start and every call ([`guard`],
//!   [`CuadConfig`]);
//! - the links over docket's and almanac's transports: the router's gate ([`IntentsGate`]), the
//!   message path and the record sink ([`DaemonLinks`], [`record_of`]);
//! - the person's settings, read from text ([`read`], [`Locator`]) and followed through a
//!   [`SettingsFeed`] the caller supplies.
//!
//! `cuad` is the daemon over this crate: the system seams, the D-Bus service and the file watch.
//!
//! # A supervisor over the fakes
//!
//! ```
//! use cua_desktop::BackendKind;
//! use cua_fake::FakePdp;
//! use cua_fake::scene::world;
//! use cua_host::{CuadConfig, Supervisor};
//! use cua_run::{CuaConfig, Enabled};
//!
//! let config = CuadConfig {
//!     settings: CuaConfig {
//!         enabled: Enabled::On,
//!         ..CuaConfig::default()
//!     },
//!     ..CuadConfig::default()
//! };
//! let parts = world(BackendKind::KwinStock, Vec::new(), FakePdp::allow_all());
//! let supervisor = Supervisor::new(parts, config);
//! assert_eq!(supervisor.settings().enabled, Enabled::On);
//! assert_eq!(supervisor.started(), 0);
//! assert!(supervisor.runs().is_empty());
//! ```

mod clock;
mod config;
mod drive;
mod events;
mod gate;
mod guard;
mod links;
mod lock;
mod settings;
mod supervisor;

pub use clock::SystemClock;
pub use config::{A11yChoice, BackendChoice, ConfigError, CuadConfig};
pub use drive::Drivable;
pub use events::{RunView, SupervisorEvent};
pub use gate::{IntentsCheck, IntentsGate};
pub use guard::guard;
pub use links::{DaemonLinks, RecordContext, RecordError, record_of};
pub use settings::{
    AppAllow, AppRule, Apps, CuaSettings, Fallback, Loaded, Locator, SCHEMA, SETTINGS_FILE,
    SettingsFeed, Why, is_browser, read,
};
pub use supervisor::{Supervisor, mint_run_id};
