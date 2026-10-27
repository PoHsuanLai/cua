//! cuad: the computer-use daemon.
//!
//! `--write-schema <dir>` writes the settings schema (`cua.settings.toml`) there and stops.
//! It reads `--config <path>` (or `$CUAD_CONFIG`; no file means the defaults, with `cua.enabled`
//! off) and runs until the bus goes away or it is sent SIGTERM. Exit code 0 is a clean stop; 2 is
//! a daemon that could not start. This file only passes the process's arguments and environment
//! in; the start-up is `cuad::run`.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match cuad::run(&args, |key| std::env::var(key).ok()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cuad: {error}");
            ExitCode::from(2)
        }
    }
}
