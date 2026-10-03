//! The run machine: `(Run, RunInput) -> (Run, Vec<RunEffect>)`, pure.

pub(crate) mod control;
pub(crate) mod effect;
pub(crate) mod finish;
pub(crate) mod input;
pub(crate) mod ops;
pub(crate) mod phase;
pub(crate) mod run;
pub(crate) mod step;
pub(crate) mod track;
