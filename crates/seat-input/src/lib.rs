//! The agent's input.
//!
//! A [`CuaAction`](cua_action::CuaAction) becomes input operations ([`plan`] over a [`Keymap`]
//! and [`SeatCaps`]), and a seat ([`AgentSeat`]) delivers them and answers with an
//! [`InputReceipt`]. Window-local points map to the EI region in the `region` module (its `GlobalPoint` is named nowhere else):
//! the identity on the fork, the window origin plus the point on stock.
//!
//! Pure by default. The `ei` feature is the reis client ([`EiSeat`]).

mod caps;
#[cfg(feature = "ei")]
mod ei;
mod keymap;
mod op;
mod plan;
mod receipt;
pub mod region;
mod seat;

pub use caps::{ScrollKind, SeatCaps, TextPath};
#[cfg(feature = "ei")]
pub use ei::EiSeat;
pub use keymap::{Keymap, KeymapEntry, KeymapError, Produces, ShiftLevel};
pub use op::{InputOp, Keycode, Press};
pub use plan::{PlanError, PlannedAction, ResolvedPoints, centre_of, plan};
pub use receipt::{InputOrigin, InputReceipt};
pub use seat::{AgentSeat, Emulation, SeatError};
