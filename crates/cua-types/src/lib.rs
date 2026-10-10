//! The leaf types that cua-bus carries inside its bodies and that the capability crates produce.
//!
//! They live here so a consumer that only reads the bus bodies links serde and the desktop
//! vocabulary, not the accessibility, input or capture crates. Each capability crate re-exports
//! its types, so `a11y_tree::Role`, `seat_input::InputReceipt` and `maskcap::FrameDigest` still
//! name them.
//!
//! Nothing here runs: plain data, no behaviour.

mod digest;
mod role;
mod seat;
mod selector;

pub use digest::FrameDigest;
pub use role::Role;
pub use seat::{InputOrigin, InputReceipt, SeatError, TextPath};
pub use selector::{NameMatch, NodeSelector, SelectorStep, SlotName};
