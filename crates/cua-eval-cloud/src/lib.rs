//! The computer-use eval against a hosted model, the way a hosted model is reached from this
//! computer: through porter's inferd on the bus. The API key is held by accountd and used by
//! inferd for one turn; this process never has it, and says so ([`isolation`]).
//!
//! `inferd` refuses a computer-use session on a hosted model today (porter FINDINGS, "inferd
//! hosted"), so this does what cua-system's `InferdCuaModel` does for a local one, one layer up:
//! stoker's `CuaSession` (run by `cua_eval::live::ProviderModel`) builds each step's prompt and
//! reads the reply, and the one model turn in between is a chat turn with the screenshot inline,
//! asked of inferd as `Need::Llm` on data class `Screen` ([`provider::InferdProvider`]).

#[cfg(test)]
mod entry;
pub mod error;
pub mod isolation;
pub mod provider;
pub mod request;
pub mod watch;
