//! What the person's controls do to a run while the model is still thinking: the call is
//! dropped, the model is handed on, and nothing the run was about to do happens.

use crate::support::*;
use cua_bus::{CancelledBy, CuaCaller, CuaMember, CuaOutcome};
use cua_desktop::BackendKind;
use cua_fake::scene::{click, reply, task, world};
use cua_fake::{FakePdp, FakeSeams};
use cua_run::{CuaModel, CuaModelError, FrameAttachment, Parts, RunPhase, Seams, StepSink};
use cuad::Supervisor;
use porter_infer::{CuaBegin, CuaStepReply, CuaStepRequest, ServedBy, TokenUsage};

/// A model that never answers, and counts the sessions it was begun for.
struct Mute {
    begun: u32,
}

impl CuaModel for Mute {
    async fn begin(&mut self, _begin: CuaBegin) -> Result<(), CuaModelError> {
        self.begun += 1;
        Ok(())
    }

    async fn step(
        &mut self,
        _request: CuaStepRequest,
        _frame: FrameAttachment,
        _sink: &mut impl StepSink,
    ) -> Result<CuaStepReply, CuaModelError> {
        std::future::pending().await
    }

    fn usage(&self) -> TokenUsage {
        TokenUsage {
            input: porter_core::Tokens(0),
            output: porter_core::Tokens(0),
            cached: porter_core::Tokens(0),
        }
    }

    fn served(&self) -> Option<ServedBy> {
        None
    }
}

/// The fakes, with a model that says nothing.
struct MuteSeams;

impl Seams for MuteSeams {
    type Desktop = <FakeSeams as Seams>::Desktop;
    type Seat = <FakeSeams as Seams>::Seat;
    type Capture = <FakeSeams as Seams>::Capture;
    type A11y = <FakeSeams as Seams>::A11y;
    type Model = Mute;
    type Gate = <FakeSeams as Seams>::Gate;
    type Events = <FakeSeams as Seams>::Events;
    type Time = <FakeSeams as Seams>::Time;
}

fn mute() -> Supervisor<MuteSeams> {
    let w = world(
        BackendKind::KwinStock,
        vec![reply(click())],
        FakePdp::allow_all(),
    );
    let parts = Parts::<MuteSeams> {
        desktop: w.desktop,
        seat: w.seat,
        capture: w.capture,
        a11y: w.a11y,
        model: Mute { begun: 0 },
        gate: w.gate,
        events: w.events,
        clock: w.clock,
    };
    Supervisor::new(parts, enabled())
}

#[tokio::test(start_paused = true)]
async fn cancel_drops_the_model_call_and_the_run_ends_cancelled() {
    let sup = mute();
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::Thinking(_))).await;
    sup.control(CuaCaller::Shell, &id, CuaMember::Cancel)
        .await
        .unwrap();
    let end = ended(&mut events, &id).await;
    assert_eq!(end.outcome, CuaOutcome::Cancelled(CancelledBy::User));
    assert!(sup.runs().is_empty());
    // The lease went back and the window is free for another run, whose model call can begin
    // at once because the first let go of the model.
    let again = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &again, |p| matches!(p, RunPhase::Thinking(_))).await;
    sup.stop_all(CuaCaller::Shell).await.unwrap();
    ended(&mut events, &again).await;
}

#[tokio::test(start_paused = true)]
async fn pausing_a_run_that_is_thinking_stops_its_input_and_nothing_is_injected_after() {
    let sup = mute();
    let mut events = sup.subscribe();
    let id = sup.start(CuaCaller::Router, task()).await.unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::Thinking(_))).await;
    sup.control(CuaCaller::Shell, &id, CuaMember::Pause)
        .await
        .unwrap();
    until(&sup, &id, |p| matches!(p, RunPhase::Paused(_))).await;
    assert!(
        sup.seat()
            .events()
            .iter()
            .all(|e| !matches!(e, cua_fake::SeatEvent::Performed(..)))
    );
    sup.control(CuaCaller::Shell, &id, CuaMember::Cancel)
        .await
        .unwrap();
    assert_eq!(
        ended(&mut events, &id).await.outcome,
        CuaOutcome::Cancelled(CancelledBy::User),
        "a paused run can still be cancelled"
    );
}
