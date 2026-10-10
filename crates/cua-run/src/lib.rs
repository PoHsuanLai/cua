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
//! The machine is the phase table; [`Exec`] carries a run's effects out against the seams
//! ([`Seams`]), and a driver (cuad's supervisor, `cua-fake`'s harness) feeds the answers back.
//! Besides the machine: [`classify_effect`], [`turn`] (the model's turn from an observation, with
//! the messages that reached the run as notes), [`mode_support`], [`CuaConfig`], [`LeaseBook`] and
//! [`stalled`].
//!
//! # Stepping a run
//!
//! `Run::step` takes an input and says what to do next. Here a run is granted the person's
//! consent and the window is leased; the machine asks for the lease, then looks at the window:
//!
//! ```
//! use cua_desktop::{BackendCaps, BackendKind, EiOrigin, LeaseGrant, LeaseId, Stamp};
//! use cua_fake::scene;
//! use cua_run::{RunEffect, RunInput, RunPhase};
//! use docket_core::GrantAnswer;
//!
//! let kind = BackendKind::CosmicStock;
//! let run = scene::started(kind);
//! assert_eq!(run.phase, RunPhase::Granting);
//!
//! let (run, effects) = run.step(RunInput::Granted(GrantAnswer::Granted), Stamp(0));
//! assert_eq!(run.phase, RunPhase::Leasing);
//! assert!(effects.iter().any(|e| matches!(e, RunEffect::AcquireLease(_))));
//!
//! let grant = LeaseGrant {
//!     id: LeaseId(1),
//!     window: scene::window(),
//!     seat: BackendCaps::of(kind).seat,
//!     ei: EiOrigin::SenderSocket,
//! };
//! let (run, effects) = run.step(RunInput::Leased(grant), Stamp(5));
//! assert!(matches!(run.phase, RunPhase::Observing(_)));
//! assert!(effects.iter().any(|e| matches!(e, RunEffect::Capture { .. })));
//! ```

mod commit;
mod config;
mod effect_class;
mod exec;
mod lease_book;
mod machine;
mod mode;
mod model;
mod observation;
mod seams;
mod sensitivity;
mod shortcuts;
mod stall;
mod turn;
mod window_class;

pub use commit::{Commit, commit_of};
pub use config::{
    Autopause, CuaConfig, Days, DefaultMode, Enabled, KeepScreens, SETTING_ROWS, SettingRow,
    SettingValue, SettleLimits,
};
pub use effect_class::classify_effect;
pub use exec::{Collected, Exec, Outlet, Seam};
pub use lease_book::{LeaseBook, LeaseHeld, Slot, SlotState};
pub use machine::effect::{CuaSignal, HintSource, RunEffect};
pub use machine::input::{CheckResult, ModelReply, RunInput};
pub use machine::phase::{ActionQueue, RunPhase};
pub use machine::run::{Run, RunEnv};
pub use machine::track::{CaptureTries, Held, RunTrack, SettleWatch};
pub use mode::{choose_mode, mode_support};
pub use model::{CuaModel, CuaModelError, StepEvent, StepSink, ThoughtText};
pub use observation::{
    ContextObs, CursorObs, Focus, Observation, TreeAbsence, TreeObs, Visibility, WindowInfo,
};
pub use seams::{
    Clock, GateCheck, GateEvent, GateLink, LinkError, MessageLink, Parts, RecordSink, Seams,
};
pub use sensitivity::{FORCED_NO, FORCED_YES, WindowSensitivity, window_sensitivity};
pub use shortcuts::shortcut_hint;
pub use stall::{Repeats, Stall};
pub use turn::{FrameAttachment, note_of, turn};
pub use window_class::{window_class_in, window_class_of};
