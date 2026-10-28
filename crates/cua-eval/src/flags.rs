//! A command-line flag that cannot be used: the error both eval programs (`cua-eval` and
//! `cua-eval-cloud`) report for their flags.

/// A command-line flag that cannot be used.
#[derive(Debug, thiserror::Error)]
pub enum FlagError {
    /// A flag that takes a value was the last word.
    #[error("{0} needs a value")]
    NeedsValue(String),
    /// A flag that takes a number was given something else.
    #[error("{0} is a number")]
    NotANumber(&'static str),
    /// A flag that takes one of a few words was given another word.
    #[error("{flag} takes {allowed}, not {given}")]
    NotAllowed {
        /// The flag.
        flag: &'static str,
        /// The words it takes, as a phrase.
        allowed: &'static str,
        /// What it was given.
        given: String,
    },
    /// A word that is no flag.
    #[error("unknown flag {0}")]
    UnknownFlag(String),
}
