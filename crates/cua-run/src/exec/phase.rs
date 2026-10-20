//! The steps of taking a leased window, each with its own bounded wait and its own log line.

use crate::machine::acquire::{AcquirePhase, AcquireWhy};
use crate::seams::Clock;
use prov::RunId;
use std::fmt;
use std::future::Future;
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

/// How long one step of taking a window may take before it is given up on.
pub(super) const PHASE_LIMIT: Duration = Duration::from_secs(20);
/// How many steps there are to take (the lease, the input, the capture).
const STEPS: u64 = 3;
/// What the whole taking is allowed: every step at its limit, and a margin. The driver's
/// backstop; each step gives up on its own long before.
pub(super) const ACQUIRE_LIMIT: Duration = Duration::from_secs(PHASE_LIMIT.as_secs() * STEPS + 10);

/// A step of taking the window, as the log names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// The desktop's lease.
    Lease,
    /// The window's input and capture.
    Phase(AcquirePhase),
}

impl fmt::Display for Step {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Step::Lease => "lease",
            Step::Phase(AcquirePhase::Attach) => "attach",
            Step::Phase(AcquirePhase::Capture) => "capture",
        })
    }
}

/// What happened to a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Progress {
    /// It began.
    Started,
    /// It finished, after this many milliseconds.
    Finished(u64),
    /// It failed, after this many milliseconds.
    Failed(u64, AcquireWhy),
}

/// One line of the log of taking a window: the run, the step, what happened. Only the run's id
/// and technical error words, never what the window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepNote {
    /// The run.
    pub run: RunId,
    /// The step.
    pub step: Step,
    /// What happened.
    pub progress: Progress,
}

impl fmt::Display for StepNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (run, step) = (&self.run, self.step);
        match &self.progress {
            Progress::Started => write!(f, "run {run}: acquire {step}: started"),
            Progress::Finished(ms) => write!(f, "run {run}: acquire {step}: finished in {ms} ms"),
            Progress::Failed(ms, AcquireWhy::TimedOut) => {
                write!(f, "run {run}: acquire {step}: timed out after {ms} ms")
            }
            Progress::Failed(ms, AcquireWhy::Failed(error)) => {
                write!(
                    f,
                    "run {run}: acquire {step}: failed after {ms} ms: {error}"
                )
            }
        }
    }
}

/// What a bounded wait came to.
pub(super) enum Bound<T> {
    /// The work finished.
    Done(T),
    /// The limit came first.
    TimedOut,
}

/// `work`, given up on after `limit` on `clock`. The work is polled first, so a clock that
/// never fires (a harness) never cuts anything short.
pub(super) async fn bounded<T>(
    clock: &impl Clock,
    limit: Duration,
    work: impl Future<Output = T>,
) -> Bound<T> {
    let mut work = pin!(work);
    let mut timer = pin!(clock.after(limit));
    std::future::poll_fn(|cx| {
        if let Poll::Ready(value) = work.as_mut().poll(cx) {
            return Poll::Ready(Bound::Done(value));
        }
        timer.as_mut().poll(cx).map(|()| Bound::TimedOut)
    })
    .await
}
