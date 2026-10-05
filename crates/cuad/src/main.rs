//! cuad: the computer-use daemon.
//!
//! It reads `--config <path>` (or `$CUAD_CONFIG`; no file means the defaults, with `cua.enabled`
//! off), connects to the session bus, tries the compositor (no compositor is not a failure: the
//! backend reads `none`), serves `org.quire.Cua1` and `org.quire.Cua`, and runs until the bus
//! goes away or it is sent SIGTERM. Exit code 0 is a clean stop; 2 is a daemon that could not
//! start.

use cuad::{
    CosmicEiSockets, CuadConfig, Links, ServeError, Supervisor, SystemClock, SystemSeams,
    WaylandPair, serve_on,
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
    let config = load_config()?;
    let connection = zbus::Connection::session()
        .await
        .map_err(|e| Fatal::Bus(e.to_string()))?;
    let wayland = match WaylandPair::connect_env() {
        Ok(pair) => Some(pair),
        Err(error) => {
            eprintln!("cuad: no compositor ({error}); the backend is unavailable");
            None
        }
    };
    let links = Links::over(&connection, config.tier);
    let sockets = CosmicEiSockets::over(connection.clone());
    let parts = SystemSeams::parts(&config, links, wayland, sockets, SystemClock::start());
    let supervisor = Arc::new(Supervisor::new(parts, config));
    let _serving = serve_on(&connection, supervisor).await?;

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
