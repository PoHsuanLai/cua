//! The lease and what hangs on it: the seat, the capture session, the model's session, input.

use super::cost::reply_cost;
use super::{Exec, ModelSession, Outlet};
use crate::machine::input::{ModelReply, RunInput};
use crate::model::{CuaModel, CuaModelError, StepEvent, StepSink};
use crate::seams::{GateCheck, Seams};
use crate::turn::FrameAttachment;
use cua_desktop::{
    DesktopError, GlowState, LeaseControl, LeaseGrant, LeaseId, LeaseRequest, Suspend,
};
use maskcap::WindowCapture;
use porter_infer::{CuaBegin, CuaStepRequest};
use seat_input::{AgentSeat, Emulation, InputOp, PlannedAction, SeatError, plan};

/// The lease a run holds, and the capture session over its window.
pub(super) struct Held<S: Seams> {
    pub grant: LeaseGrant,
    pub capture: <S::Capture as WindowCapture>::Session,
}

/// Forwards what a model says while it thinks to the run, as it says it.
struct Forward<'o, O: Outlet>(&'o mut O);

impl<O: Outlet> StepSink for Forward<'_, O> {
    fn event(&mut self, event: StepEvent) {
        self.0.input(RunInput::ModelEvent(event));
    }
}

impl<S: Seams> Exec<'_, S> {
    pub(super) async fn acquire(&mut self, request: LeaseRequest, out: &mut impl Outlet) {
        let leased = match self.seam.desktop.lease(request).await {
            Ok(leased) => leased,
            Err(error) => return out.input(RunInput::LeaseRefused(error)),
        };
        let grant = leased.grant;
        if let Err(error) = self.seam.seat.attach(&grant, leased.socket).await {
            let _ = self.seam.desktop.release(grant.id).await;
            return out.input(RunInput::LeaseRefused(DesktopError::Protocol(
                error.to_string(),
            )));
        }
        match self.seam.capture.open(&grant).await {
            Ok(capture) => {
                self.held = Some(Held {
                    grant: grant.clone(),
                    capture,
                });
                out.input(RunInput::Leased(grant));
            }
            Err(error) => {
                let _ = self.seam.seat.detach(grant.id).await;
                let _ = self.seam.desktop.release(grant.id).await;
                out.input(RunInput::LeaseRefused(DesktopError::Protocol(
                    error.to_string(),
                )));
            }
        }
    }

    pub(super) async fn release(&mut self, lease: LeaseId) {
        self.check = None;
        let _ = self.seam.seat.detach(lease).await;
        let _ = self.seam.desktop.release(lease).await;
        self.held = None;
    }

    /// Input stops: the desktop's lease is suspended and, on a shared seat, so is our own
    /// emulation (the seat is the person's too).
    pub(super) async fn suspend(&mut self, lease: LeaseId, why: Suspend) {
        if why == Suspend::Paused
            && let Some(mut check) = self.check.take()
        {
            // A paused run withdraws its question: the sheet comes down.
            let _ = check.close().await;
        }
        let _ = self.seam.seat.emulation(lease, Emulation::Stopped).await;
        let _ = self.seam.desktop.suspend(lease, why).await;
    }

    pub(super) async fn resume(&mut self, lease: LeaseId) {
        let _ = self.seam.desktop.resume(lease).await;
        let _ = self.seam.seat.emulation(lease, Emulation::Running).await;
    }

    pub(super) async fn glow(&mut self, glow: GlowState) {
        let Some(lease) = self.held.as_ref().map(|held| held.grant.id) else {
            return;
        };
        let _ = self.seam.desktop.glow(lease, glow).await;
    }

    pub(super) async fn inject(&mut self, planned: PlannedAction, out: &mut impl Outlet) {
        let Some(lease) = self.held.as_ref().map(|held| held.grant.id) else {
            return out.input(RunInput::InjectFailed(SeatError::NotConnected));
        };
        let keymap = self.seam.seat.keymap();
        let ops: Vec<InputOp> = match plan(&planned, &keymap, &self.seam.seat.caps()) {
            Ok(ops) => ops,
            Err(seat_input::PlanError::OutsideWindow(_)) => {
                return out.input(RunInput::InjectFailed(SeatError::OutsideLease));
            }
            Err(other) => {
                return out.input(RunInput::InjectFailed(SeatError::Keymap(other.to_string())));
            }
        };
        out.input(match self.seam.seat.perform(lease, &ops).await {
            Ok(receipt) => RunInput::Injected(receipt),
            Err(error) => RunInput::InjectFailed(error),
        });
    }

    /// The model's turn: its session is opened with the goal on the first one.
    pub(super) async fn call_model(
        &mut self,
        model: &mut S::Model,
        request: CuaStepRequest,
        frame: FrameAttachment,
        out: &mut impl Outlet,
    ) {
        if self.model == ModelSession::NotYet {
            let begin = CuaBegin::new(
                self.task.goal.value.clone(),
                porter_core::capability::CuaEnv::Desktop,
            )
            .with_hints(self.hints.clone());
            if let Err(error) = model.begin(begin).await {
                return out.input(RunInput::ModelFailed(error));
            }
            self.model = ModelSession::Open;
        }
        let reply = model.step(request, frame, &mut Forward(out)).await;
        if let Some(served) = model.served()
            && self.announced.as_ref() != Some(&served)
        {
            self.announced = Some(served.clone());
            out.input(RunInput::Routed(served));
        }
        out.input(match reply {
            Ok(reply) => RunInput::ModelReplied(Box::new(ModelReply {
                reply,
                usage: model.usage(),
                cost: reply_cost(model.served().as_ref()),
            })),
            Err(
                error @ (CuaModelError::Transient | CuaModelError::Parse | CuaModelError::Fatal),
            ) => RunInput::ModelFailed(error),
        });
    }
}
