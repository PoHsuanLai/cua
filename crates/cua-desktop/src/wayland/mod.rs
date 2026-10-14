//! The wayland plumbing a compositor backend shares: [`Pump`] drives one connection from async
//! code, and [`ObjectKey`], [`Rounds`] and [`SyncTag`] are the keys and round-trip counters a
//! backend's handlers fill.

mod key;
mod pump;

pub use key::{ObjectKey, Rounds, SyncTag};
pub use pump::{Pump, PumpError};
