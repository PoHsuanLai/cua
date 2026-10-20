//! cuad: the computer-use daemon.
//!
//! `--write-schema <dir>` writes the settings schema (`cua.settings.toml`) there and stops.
//! It reads `--config <path>` (or `$CUAD_CONFIG`; no file means the defaults, with `cua.enabled`
//! off), connects to the session bus, tries the compositor (no compositor is not a failure: the
//! backend reads `none`), serves `org.quire.Cua1` and `org.quire.Cua`, and runs until the bus
//! goes away or it is sent SIGTERM. Exit code 0 is a clean stop; 2 is a daemon that could not
//! start.

use cuad::{
    Apps, CuaSettings, CuadConfig, Links, Locator, ReachError, Reached, ServeError, SettingsWatch,
    Supervisor, SystemClock, SystemSeams, WatchState, caller_identity, follow_settings,
    read_callers, serve_on_as,
};
use futures_util::StreamExt;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use tokio::signal::unix::{SignalKind, signal};

/// Why the daemon did not start or stopped badly.
#[derive(Debug, thiserror::Error)]
enum Fatal {
    #[error("{0}")]
    Config(String),
    #[error("session bus: {0}")]
    Bus(String),
    #[error("compositor: {0}")]
    Compositor(#[from] ReachError),
    #[error("{0}")]
    Serve(#[from] ServeError),
    #[error("signal handler: {0}")]
    Signal(String),
}

/// The configuration file named on the command line or in the environment, if any.
fn config_path() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
    }
    std::env::var_os("CUAD_CONFIG").map(PathBuf::from)
}

/// `--write-schema DIR`: where to write `cua.settings.toml` for a local install (design/22
/// section 9.2), when asked.
fn schema_dir() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--write-schema" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn write_schema(dir: &std::path::Path) -> Result<(), Fatal> {
    let io = |e: std::io::Error| Fatal::Config(format!("{}: {e}", dir.display()));
    std::fs::create_dir_all(dir).map_err(io)?;
    std::fs::write(dir.join("cua.settings.toml"), cuad::SCHEMA).map_err(io)
}

fn load_config() -> Result<CuadConfig, Fatal> {
    let Some(path) = config_path() else {
        return Ok(CuadConfig::default());
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => CuadConfig::from_toml(&text).map_err(|e| Fatal::Config(e.to_string())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CuadConfig::default()),
        Err(e) => Err(Fatal::Config(format!("{}: {e}", path.display()))),
    }
}

async fn run() -> Result<(), Fatal> {
    if let Some(dir) = schema_dir() {
        return write_schema(&dir);
    }
    let config = load_config()?;
    // The desktop's shortcut conventions, read once and told to every run's model.
    let env = |key: &str| std::env::var(key).ok();
    let platform = chordkit::detect(chordkit::HostOs::compiled(), env);
    let (keymap, problems) = cuad::desktop_keymap(platform, env);
    for problem in problems {
        eprintln!("cuad: {problem}");
    }
    let hints: Vec<String> = cua_run::shortcut_hint(&keymap).into_iter().collect();
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| Fatal::Bus(e.to_string()))?;
    let reached = Reached::find(&config, &connection).await?;
    eprintln!("cuad: {}", reached.describe());
    let links = Links::over(&connection, config.tier);
    let parts = SystemSeams::parts(&config, links, reached, SystemClock::start());
    let supervisor = Arc::new(Supervisor::with_hints(parts, config, hints));
    // The person's settings (`cua/settings.toml`) over the configuration's values, now and
    // whenever the file changes. Until the file lists an app, none is on.
    let base = CuaSettings {
        config: supervisor.settings(),
        apps: Apps::Listed(Vec::new()),
    };
    let mut watched = SettingsWatch::start(Locator::from_env(&env), base);
    if let WatchState::Blind { reason } = watched.state() {
        eprintln!("cuad: settings are read once, not watched: {reason}");
    }
    let first = watched.current();
    for line in first.lines("cuad") {
        eprintln!("{line}");
    }
    supervisor.apply_settings(&first.value);
    let following = supervisor.clone();
    let announcing = connection.clone();
    tokio::spawn(async move { follow_settings(&announcing, &following, &mut watched).await });
    // Who is calling is the process behind the connection, named by the callers table: the
    // system's `cuad-callers.toml` with the person's laid over it.
    let table = read_callers(&env).map_err(|e| Fatal::Config(e.to_string()))?;
    let who = caller_identity(connection.clone(), table, &env);
    let _serving = serve_on_as(&connection, supervisor, who).await?;

    let mut terminate =
        signal(SignalKind::terminate()).map_err(|e| Fatal::Signal(e.to_string()))?;
    let mut messages = zbus::MessageStream::from(&connection);
    tokio::select! {
        () = async { while messages.next().await.is_some() {} } => {}
        _ = terminate.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cuad: {error}");
            ExitCode::from(2)
        }
    }
}
