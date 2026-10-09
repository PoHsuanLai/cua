//! Test support for cua. Nothing here is product code: it exists so every other crate's tests
//! run without a compositor, a seat, a bus or a model.
//!
//! - [`FakeSeat`] records input; [`FakeEis`] is an EIS server in the test process (over reis);
//! - [`FakeCapture`] returns scripted frames and damage; [`FakeDesktop`] has windows, events and
//!   the capabilities of a backend; [`FakeA11y`] answers with the a11y-tree fixtures;
//! - [`ScriptedModel`] says what it was scripted to say;
//! - [`FakePdp`] is the router's gate as a table; [`MemoryEvents`] keeps the event log and the
//!   `prov::Message` reports a run sends; [`FixedClock`] moves only when told;
//! - [`run_harness`] drives a run over all of them.

mod a11y;
mod capture;
mod clock;
mod desktop;
mod eis;
mod eis_keymap;
mod events;
mod gate;
mod harness;
mod model;
pub mod scene;
mod seat;

pub use a11y::{FakeA11y, Fixture};
pub use capture::{FakeCapture, FakeCaptureSession};
pub use clock::FixedClock;
pub use desktop::{DesktopCall, FakeDesktop};
pub use eis::{EisEvent, FakeEis};
pub use events::MemoryEvents;
pub use gate::{FakeCheck, FakePdp, Verdict};
pub use harness::{
    FakeSeams, HarnessError, HarnessReport, World, run_harness, run_harness_answering,
};
pub use model::{ScriptError, ScriptedModel};
pub use seat::{FakeSeat, SeatEvent};
