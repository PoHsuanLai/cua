//! Where the settings text comes from. The host never opens a file: the caller hands it a
//! [`ReadText`], so the lookup runs over plain values in a test and over the disk in the daemon.

use std::path::Path;

/// Reads the text of one file.
pub trait ReadText {
    /// The text at `path`.
    ///
    /// # Errors
    ///
    /// [`ReadFailure::Missing`] when there is no such file, [`ReadFailure::Unreadable`] when there
    /// is one and it cannot be read.
    fn read_text(&self, path: &Path) -> Result<String, ReadFailure>;
}

/// Why a file gave no text.
#[derive(Debug, thiserror::Error)]
pub enum ReadFailure {
    /// There is no file. The ordinary case for a configuration directory.
    #[error("no such file")]
    Missing,
    /// The file is there and cannot be read.
    #[error("unreadable: {source}")]
    Unreadable {
        /// What the disk said.
        #[source]
        source: std::io::Error,
    },
}
