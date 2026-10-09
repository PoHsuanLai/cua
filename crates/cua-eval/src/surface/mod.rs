//! Fake applications: deterministic surfaces a computer-use model can be played against without a
//! desktop, an app or an account.

pub mod def;
pub mod render;
pub mod state;

pub use def::{Act, Cond, SurfaceDef};
pub use render::render;
pub use state::{Hit, Stroke, Surface, SurfaceError};
