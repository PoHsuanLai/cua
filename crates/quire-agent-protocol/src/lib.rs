//! The paths cua has always used for `quire-agent-v1`, now served by `casement-protocols`.
//!
//! The XML and its wayland-scanner bindings live in the `casement-protocols` repo, the one home
//! of every `quire_*_v1` protocol. This crate only re-exports them under the names cua's crates
//! already import (`client::quire_agent_manager_v1`, `client::quire_agent_lease_v1`,
//! `v1::client`), so none of them moved. The feature `client` is cuad's side; `server` is the
//! compositor's.

pub use casement_protocols::vocab;

pub mod v1 {
    //! `quire-agent-v1`, under its original module path.
    #[cfg(feature = "client")]
    pub use casement_protocols::client::quire_agent_v1 as client;
    #[cfg(feature = "server")]
    pub use casement_protocols::server::quire_agent_v1 as server;
}

#[cfg(feature = "client")]
pub use casement_protocols::client::quire_agent_v1 as client;
#[cfg(feature = "server")]
pub use casement_protocols::server::quire_agent_v1 as server;
