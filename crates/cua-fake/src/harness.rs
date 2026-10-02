//! A run driven over the fakes: the machine's effects carried out against them, and their
//! answers fed back, until the run ends.

use crate::a11y::FakeA11y;
use crate::capture::FakeCapture;
use crate::clock::FixedClock;
use crate::desktop::FakeDesktop;
use crate::events::MemoryEvents;
use crate::gate::FakePdp;
use crate::model::ScriptedModel;
use crate::seat::FakeSeat;
use cua_run::{CuaSignal, Parts, Run, RunPhase, Seams};

/// The bundle of fakes: every seam of a run, faked.
#[derive(Debug)]
pub struct FakeSeams;

impl Seams for FakeSeams {
    type Desktop = FakeDesktop;
    type Seat = FakeSeat;
    type Capture = FakeCapture;
    type A11y = FakeA11y;
    type Model = ScriptedModel;
    type Gate = FakePdp;
    type Events = MemoryEvents;
    type Time = FixedClock;
}

/// Every fake a run needs, in one place.
pub type World = Parts<FakeSeams>;

/// What a harness run leaves behind. The records and the reports sent are in `World::events`.
#[derive(Debug)]
pub struct HarnessReport {
    /// The run, as it ended.
    pub run: Run,
    /// The signals, in order.
    pub signals: Vec<CuaSignal>,
}

/// Why a harness run did not end.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HarnessError {
    /// The run is waiting for something no fake will ever send.
    #[error("stuck in {0:?}")]
    Stuck(RunPhase),
    /// More steps than the script allows for.
    #[error("too many steps")]
    TooManySteps,
}

/// Drives `run` to its end over `world`: carry out each effect against the fake that owns it,
/// feed the answer back as a `RunInput`, advance the clock only when a fake says time passes.
pub async fn run_harness(run: Run, world: &mut World) -> Result<HarnessReport, HarnessError> {
    let _ = (run, world);
    todo!(
        "end_to_end_scripted_run_done: click then done over ScriptedModel, FakeSeat, FakeCapture and FakePdp"
    )
}
