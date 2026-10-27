//! The system adapters of computer use: the real seams of a running desktop, in a library an
//! app or a daemon can use.
//!
//! - [`SystemSeams`] is the bundle of seams `cua_run` runs against. [`SystemSeams::parts`] builds
//!   it over the [`Links`] to the daemons (the router's gate, memoryd, inferd) and what was
//!   [`Reached`] of a compositor.
//! - The backends are closed sets: [`SeatBackend`] (`Ei`), [`CaptureBackend`] (`StockToplevel`,
//!   `QuireMasked`), [`SystemDesktop`] (`Casement`, or `Absent`) and [`SystemA11y`] (AT-SPI, or
//!   none). With no compositor the unavailable cases answer, and the bus says `Backend = none`.
//! - [`CasementDesktop`] is the desktop of casement: windows from the agent connection and a lease
//!   per window from `Compositor1`.
//! - [`InferdCuaModel`] is the computer-use model over an inferd session, and [`frame_fd`] the
//!   sealed memfd a frame rides in.
//!
//! The backends themselves are in `seat-ei`, `maskcap-wayland` and `a11y-atspi`.

mod backend;
mod casement;
mod connect;
mod desktop;
mod frame_fd;
mod model;
mod system;

pub use backend::{CaptureBackend, CaptureKind, CaptureSessionKind, SeatBackend, capture_kind};
pub use casement::{CasementDesktop, Door};
pub use connect::Links;
pub use desktop::{SystemA11y, SystemDesktop};
pub use frame_fd::{FrameFdError, frame_fd};
pub use model::InferdCuaModel;
pub use system::{ReachError, Reached, SystemSeams, WaylandPair};
