//! cuad: the computer-use daemon, as a library and a binary.
//!
//! It owns the step loop of every run: the pure machine ([`cua_run::Run`]) says what to do, the
//! [`Supervisor`] does it against the seams (`cua_system::SystemSeams` in the daemon, `cua-fake`
//! in tests) and feeds the answers back. It serves `org.quire.Cua1` (`cua-dbus`) and
//! `org.quire.IntentProvider1` as `org.quire.Cua` with one action, `cua.run.start`
//! ([`CuaProvider`]).
//!
//! On a stock compositor masking is cuad's (`MaskReport::applied_by = Cuad`) and the consent
//! guard is cuad stopping its own input; casement P4 moves both into the compositor.
//!
//! A run's progress and result go to the requester as `prov::Message` reports through the
//! router. A message that arrives carries no authority.
//!
//! What it holds: the tables ([`guard`], [`classify`], [`manifest_for`], [`record_of`],
//! [`CuadConfig::from_toml`]), the supervisor ([`Supervisor`]: a task per run over
//! [`cua_run::Exec`], with the person's controls, the desktop's events and the clock racing
//! whatever the run waits on), the bus ([`serve`], [`serve_on`]) and the start-up ([`run`]). The
//! adapters to the system (input over EIS, window capture, the casement desktop, the model over
//! inferd, the links to the router and memoryd) are in `cua-system`; this crate wires them. With
//! no compositor it still starts and reports the backend as unavailable (`Backend = none`).

mod bus;
mod callers;
mod daemon;
mod keymap;
mod provider;
mod reach;
mod reasons;
mod relay;
mod serve;
mod start;
mod watch;

pub use callers::{
    FILE as CALLERS_FILE, SHELL_BUS, classify, identity as caller_identity, read as read_callers,
    shipped as shipped_callers,
};
pub use cua_host::{
    A11yChoice, AppAllow, AppRule, Apps, BackendChoice, ConfigError, CuaSettings, CuadConfig,
    DaemonLinks, Drivable, Fallback, IntentsCheck, IntentsGate, Loaded, Locator, RecordContext,
    RecordError, RunView, SCHEMA, SETTINGS_FILE, SettingsFeed, Supervisor, SupervisorEvent,
    SystemClock, Why, guard, is_browser, mint_run_id, read, record_of,
};
pub use daemon::{Fatal, run};
pub use keymap::desktop_keymap;
pub use provider::{
    CuaProvider, ModeChoice, ProviderError, StartRefusal, StartRequest, manifest_for,
};
pub use reach::{follow_settings, reach_json};
pub use serve::{ServeError, Serving, serve, serve_on, serve_on_as, serve_on_with};
pub use watch::{DEBOUNCE, SettingsWatch, WatchState};
