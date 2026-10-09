//! The computer-use model eval.
//!
//! A task is played on a fake surface (`surface`): a small deterministic application that is
//! drawn as pixels for the model and acted on through the same seat, capture and desktop seams a
//! real run uses, with cua's own run machine in between (`run`). The model is a script (`script`)
//! or a real one on a local engine (`live`). A reference gate (`gate`) stands in for the router.
//! What comes out is a verdict per run (`verdict`), rates with bounds (`metrics`) and a markdown
//! report (`report`). Nothing here touches the desktop, an app, an account or the network.

pub mod corpus;
pub mod eval;
mod font_data;
pub mod gate;
pub mod keys;
pub mod live;
pub mod metrics;
pub mod report;
pub mod run;
pub mod script;
pub mod stage;
pub mod surface;
pub mod task;
pub mod verdict;
pub mod world;
