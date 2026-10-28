//! The system adapters of computer use: the real seams of a running desktop, in a library an
//! app or a daemon can use.
//!
//! - [`SystemSeams`] is the bundle of seams `cua_run` runs against. [`SystemSeams::parts`] builds
//!   it over the [`Links`] to the daemons (the router's gate, memoryd, inferd) and what was
//!   [`Reached`] of a compositor.
//! - The seat is `EiSeat`. Capture is a [`CaptureBackend`] (`StockToplevel`, `QuireMasked`), the
//!   desktop a [`CasementDesktop`] and the tree a [`LoggedAtspi`] (AT-SPI), each inside an `Option`.
//!   With no compositor the `None` cases answer, and the bus says `Backend = none`.
//! - [`CasementDesktop`] is the desktop of casement: windows from the agent connection and a lease
//!   per window from `Compositor1`.
//! - [`InferdCuaModel`] is the computer-use model over an inferd session, and [`frame_fd`] the
//!   sealed memfd a frame rides in.
//!
//! The backends themselves are in `seat-ei`, `maskcap-wayland` and `a11y-atspi`.

mod a11y;
mod backend;
mod casement;
mod connect;
mod frame_fd;
mod model;
mod system;

pub use a11y::LoggedAtspi;
pub use backend::CaptureBackend;
pub use casement::{CasementDesktop, Door};
pub use connect::Links;
pub use frame_fd::{FrameFdError, frame_fd};
pub use model::InferdCuaModel;
pub use system::{ReachError, Reached, SystemSeams, WaylandPair};
