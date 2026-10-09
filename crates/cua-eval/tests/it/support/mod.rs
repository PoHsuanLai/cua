//! Shared builders: the task set on disk, and a way to run a future that never waits on a clock.

use cua_eval::corpus::Corpus;
use std::path::PathBuf;

/// The task set under `eval/` at the root of the workspace.
pub fn eval_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../eval")
}

pub fn corpus() -> Corpus {
    Corpus::load(&eval_dir()).expect("the task set loads")
}

/// Runs `future` on a runtime of its own.
pub fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
        .block_on(future)
}
