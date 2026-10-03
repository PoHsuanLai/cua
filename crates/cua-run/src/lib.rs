//! The pure core of a computer-use run.
//!
//! [`Run`] is a state machine like docket's `agent-loop`: `Run::step(self, input, at)` returns the
//! next run and the [`RunEffect`]s to carry out; cuad carries them out and feeds back the
//! [`RunInput`]s. Everything here is `Serialize + Eq`, so a run can be written to a trace and
//! replayed, and nothing here touches a window, a socket or the clock.
//!
//! The phase table is `cua.md` section 4, changed where docket's gate changed it: a pixel step
//! is authorised by `Intents1.Gate.Check` ([`RunEffect::Check`]); when the router will ask the
//! person it says so first ([`RunInput::Confirming`]), the machine stops the lease on stock and
//! calls `Proceed`, and the router's final answer arrives as [`RunInput::Verdict`].
//!
//! **One message model.** A run reports to its requester with [`RunEffect::Report`]: a draft of
//! kind `Report` whose status is `Progress` while it waits for the person and `Done`, `Failed` or
//! `Cancelled` at the end. [`RunInput::Inbound`] carries the messages that arrive for it; a
//! message carries no authority and widens nothing.
//!
//! What is real here is the vocabulary and its tables ([`mode_support`], [`CuaConfig`], the
//! phase facts); the machine's transitions, [`classify_effect`], [`turn`], [`LeaseBook`] and
//! [`stalled`] are frozen `todo!()` signatures.

mod config;
mod effect_class;
mod lease_book;
mod machine;
mod mode;
mod model;
mod observation;
mod seams;
mod stall;
mod turn;
mod window_class;

pub use config::{
    Autopause, CuaConfig, Days, DefaultMode, Enabled, KeepScreens, SETTING_ROWS, SettingRow,
    SettingValue, SettleLimits,
};
pub use effect_class::classify_effect;
pub use lease_book::{LeaseBook, LeaseHeld, Slot, SlotState};
pub use machine::effect::{CuaSignal, HintSource, RunEffect};
pub use machine::input::{CheckResult, ModelReply, RunInput};
pub use machine::phase::{ActionQueue, RunPhase};
pub use machine::run::{Run, RunEnv};
pub use machine::track::{CaptureTries, RunTrack, SettleWatch};
pub use mode::{choose_mode, mode_support};
pub use model::{CuaModel, CuaModelError, StepEvent, StepSink, ThoughtText};
pub use observation::{
    ContextObs, CursorObs, Focus, Observation, TreeAbsence, TreeObs, Visibility, WindowInfo,
};
pub use seams::{
    Clock, GateCheck, GateEvent, GateLink, LinkError, MessageLink, Parts, RecordSink, Seams,
};
pub use stall::{Stall, stalled};
pub use turn::{FrameAttachment, note_of, turn};
pub use window_class::window_class_of;
