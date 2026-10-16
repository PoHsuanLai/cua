//! How long an effect may take, and what the machine is told when it takes longer.
//!
//! Every effect that talks to a neighbour (the desktop, the seat, capture, the model, the router,
//! the log) has a deadline, so a stalled neighbour fails a step instead of freezing the run. The
//! driver races the effect against [`Exec::deadline_of`]; the effects that wait on the person
//! (the per-app consent, a confirmation) have none, and the active budget does not count them
//! either.

use super::Exec;
use crate::machine::effect::RunEffect;
use crate::machine::input::{CheckResult, RunInput};
use crate::model::CuaModelError;
use crate::screen::ScreenRead;
use crate::seams::{LinkError, Seams};
use cua_desktop::DesktopError;
use maskcap::CaptureError;
use seat_input::SeatError;
use std::time::Duration;

/// A neighbour that should answer at once: the desktop, the seat, a capture, the tree.
const QUICK: Duration = Duration::from_secs(30);
/// The router's gate.
const GATE: Duration = Duration::from_secs(60);
/// The model: a step with a large frame on a loaded engine takes a while.
const MODEL: Duration = Duration::from_secs(180);

/// When an effect is given up on, and what the machine hears then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    /// How long the effect may run.
    pub after: Duration,
    /// The input that stands for its failure; none when the effect has no answer to give.
    pub failure: Option<RunInput>,
}

fn timed_out() -> String {
    "timed out".to_owned()
}

/// The deadline of `effect`; none for an effect that finishes at once or waits on the person.
pub fn deadline_of(effect: &RunEffect) -> Option<Deadline> {
    let (after, failure) = match effect {
        RunEffect::AcquireLease(_) => (
            QUICK,
            Some(RunInput::LeaseRefused(DesktopError::Protocol(timed_out()))),
        ),
        RunEffect::Capture { .. } => (
            QUICK,
            Some(RunInput::CaptureFailed(CaptureError::Protocol(timed_out()))),
        ),
        RunEffect::CallModel { .. } => {
            (MODEL, Some(RunInput::ModelFailed(CuaModelError::Transient)))
        }
        RunEffect::ReadScreen(_) => (QUICK, Some(RunInput::ScreenRead(ScreenRead::Unavailable))),
        RunEffect::Check(_) => (GATE, Some(RunInput::GateFailed(LinkError::Unavailable))),
        RunEffect::Inject(_) => (
            QUICK,
            Some(RunInput::InjectFailed(SeatError::Protocol(timed_out()))),
        ),
        RunEffect::VerifyTree(_) => (QUICK, Some(RunInput::Checked(CheckResult::Fail))),
        RunEffect::Snapshot(_)
        | RunEffect::ReleaseLease(_)
        | RunEffect::SuspendLease(..)
        | RunEffect::ResumeLease(_)
        | RunEffect::Glow(_)
        | RunEffect::Record(_)
        | RunEffect::Report(_) => (QUICK, None),
        // The consent and a confirmation wait on the person; the rest are instant.
        _ => return None,
    };
    Some(Deadline { after, failure })
}

impl<S: Seams> Exec<'_, S> {
    /// The model's session is no longer this run's (the driver let the model go while the run
    /// waited): the next model call opens a new one.
    pub fn forget_model_session(&mut self) {
        self.model = super::ModelSession::NotYet;
    }
}
