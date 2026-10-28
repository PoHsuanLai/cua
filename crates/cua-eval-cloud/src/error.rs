//! Why the `cua-eval-cloud` program stops: a flag it cannot use, or a step of the set-up or the run
//! that failed. The binary prints the error and exits with status 2.

use cua_eval::corpus::CorpusError;
use cua_eval::flags::FlagError;
use cua_eval::live::LiveError;
use cua_eval::run::SetupError;
use model_catalog::CatalogError;
use porter_client::TransportError;
use porter_core::{CoreError, Tier};
use std::path::PathBuf;

/// A command-line flag that cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum ArgError {
    /// A flag that cannot be used.
    #[error(transparent)]
    Flag(#[from] FlagError),
    /// A flag the program cannot run without.
    #[error("--entry and --label are required")]
    Required,
}

/// Why the program stopped.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// A flag could not be used.
    #[error(transparent)]
    Args(#[from] ArgError),
    /// This process holds or can reach a credential.
    #[error("this process can reach a credential; refusing to run")]
    CredentialReachable,
    /// The task set did not load.
    #[error(transparent)]
    Corpus(#[from] CorpusError),
    /// The model plan could not be made from the entry.
    #[error(transparent)]
    Plan(#[from] LiveError),
    /// The catalogue entry did not parse.
    #[error("catalogue entry: {0}")]
    Catalog(#[from] CatalogError),
    /// The entry names no OpenRouter route to price.
    #[error("the entry has no OpenRouter reach")]
    NoPrice,
    /// A fixed account or model id was refused.
    #[error(transparent)]
    Id(#[from] CoreError),
    /// The session bus could not be reached.
    #[error("the bus: {0}")]
    Bus(#[from] zbus::Error),
    /// Inferd will not serve the chat the run needs.
    #[error("inferd will not serve a {tier:?} chat with images: {source}")]
    Prepare {
        /// The tier asked for.
        tier: Tier,
        /// Why not.
        source: TransportError,
    },
    /// A task could not be set up or played.
    #[error(transparent)]
    Setup(#[from] SetupError),
    /// A file could not be read or written.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// The cassette could not be written as TOML.
    #[error("cassette: {0}")]
    Cassette(#[from] toml::ser::Error),
}
