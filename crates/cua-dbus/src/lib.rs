//! cuad's D-Bus API: `org.quire.Cua1.Manager` at `/org/quire/Cua1` and `org.quire.Cua1.Run` at
//! `/org/quire/Cua1/run/<id>`. Each interface is declared twice: a proxy trait for callers (sill,
//! companiond, intentd) and a skeleton for cuad, whose introspection is the checked-in
//! `dbus/org.quire.Cua1.xml` (see `tests/introspection.rs`). Bodies are the JSON of the named
//! `cua-bus` type in an `s` argument, in the `CuaEnvelope` where a body is sent; the caller's
//! identity is derived from the connection and never sent. Signatures only: every skeleton
//! method answers `NotSupported`.
//!
//! A run's result is not a member of this interface: it is a `prov::Message` of kind `Report`
//! delivered through the router, and the `Finished` signal says which.

mod error;
mod introspect;
mod manager;
mod names;
mod run;

pub use error::CuaError;
pub use introspect::{FILE_NAME, introspection};
pub use manager::{ManagerProxy, ManagerSkeleton};
pub use names::{CUA_BUS, CUA_PATH, PROVIDER_BUS, run_path};
pub use run::{RunProxy, RunSkeleton};
