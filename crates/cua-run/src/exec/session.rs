//! The lease and what hangs on it: the seat, the capture session, the model's session, input.

use super::fit::{Fit, takes};
use super::phase::{Bound, PHASE_LIMIT, bounded};
use super::{Exec, Outlet, Progress, Step, StepNote};
use crate::machine::acquire::{AcquireFailure, AcquirePhase, AcquireWhy};
use crate::machine::input::{ModelReply, Moved, RunInput};
use crate::model::{CuaModel, CuaModelError, StepEvent, StepSink};
use crate::seams::{Clock, GateCheck, Seams};
use crate::turn::FrameAttachment;
use cua_desktop::{
    DesktopError, GlowState, LeaseControl, LeaseGrant, LeaseId, LeaseRequest, Stamp, Suspend,
};
use cua_desktop::{ToplevelIdent, ToplevelInfo};
use maskcap::{CaptureError, WindowCapture};
use porter_capability::Locality;
use porter_infer::{CuaBegin, CuaStepRequest};
use seat_input::{AgentSeat, Emulation, InputOp, PlannedAction, SeatError, plan};

/// The lease a run holds, and the capture session over its window.
pub(super) struct Held<S: Seams> {
    pub grant: LeaseGrant,
    pub capture: <S::Capture as WindowCapture>::Session,
    /// The session over the dialog the window opened, while one is in front of it.
    pub dialog: Option<Dialog<S>>,
}

/// A dialog in front of the leased window, and the capture session over it.
pub(super) struct Dialog<S: Seams> {
    ident: ToplevelIdent,
    capture: <S::Capture as WindowCapture>::Session,
}

/// Which window the capture follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Following {
    /// The leased window itself.
    Window,
    /// A dialog it opened.
    Dialog,
}

impl<S: Seams> Held<S> {
    /// The session over the window in front: the dialog's while there is one.
    pub(super) fn session(&mut self) -> &mut <S::Capture as WindowCapture>::Session {
        match &mut self.dialog {
            Some(dialog) => &mut dialog.capture,
            None => &mut self.capture,
        }
    }

    /// The dialog's session ends (it closed): the capture is back on the window.
    pub(super) fn unfollow(&mut self) {
        self.dialog = None;
    }
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
        let began = self.seam.clock.now();
        self.note(out, Step::Lease, Progress::Started);
        let leased = match bounded(
            self.seam.clock,
            PHASE_LIMIT,
            self.seam.desktop.lease(request),
        )
        .await
        {
            Bound::Done(Ok(leased)) => leased,
            Bound::Done(Err(error)) => {
                let why = AcquireWhy::Failed(error.to_string());
                self.note(out, Step::Lease, Progress::Failed(self.since(began), why));
                return out.input(RunInput::LeaseRefused(error));
            }
            Bound::TimedOut => {
                let why = Progress::Failed(self.since(began), AcquireWhy::TimedOut);
                self.note(out, Step::Lease, why);
                return out.input(RunInput::LeaseRefused(DesktopError::Protocol(
                    "timed out".to_owned(),
                )));
            }
        };
        self.note(out, Step::Lease, Progress::Finished(self.since(began)));
        let grant = leased.grant;

        let began = self.seam.clock.now();
        self.note(out, Step::Phase(AcquirePhase::Attach), Progress::Started);
        let attached = bounded(
            self.seam.clock,
            PHASE_LIMIT,
            self.seam.seat.attach(&grant, leased.socket),
        )
        .await;
        match attached {
            Bound::Done(Ok(())) => {
                let done = Progress::Finished(self.since(began));
                self.note(out, Step::Phase(AcquirePhase::Attach), done);
            }
            Bound::Done(Err(SeatError::Busy)) => {
                let why = AcquireWhy::Failed(SeatError::Busy.to_string());
                let failed = Progress::Failed(self.since(began), why);
                self.note(out, Step::Phase(AcquirePhase::Attach), failed);
                let _ = self.seam.seat.detach(grant.id).await;
                let _ = self.seam.desktop.release(grant.id).await;
                return out.input(RunInput::LeaseRefused(DesktopError::Busy));
            }
            Bound::Done(Err(error)) => {
                let why = AcquireWhy::Failed(error.to_string());
                return self
                    .abandon(&grant, AcquirePhase::Attach, began, why, out)
                    .await;
            }
            Bound::TimedOut => {
                let why = AcquireWhy::TimedOut;
                return self
                    .abandon(&grant, AcquirePhase::Attach, began, why, out)
                    .await;
            }
        }

        let began = self.seam.clock.now();
        self.note(out, Step::Phase(AcquirePhase::Capture), Progress::Started);
        match bounded(self.seam.clock, PHASE_LIMIT, self.seam.capture.open(&grant)).await {
            Bound::Done(Ok(capture)) => {
                let done = Progress::Finished(self.since(began));
                self.note(out, Step::Phase(AcquirePhase::Capture), done);
                self.held = Some(Held {
                    grant: grant.clone(),
                    capture,
                    dialog: None,
                });
                out.input(RunInput::Leased(grant));
            }
            Bound::Done(Err(error)) => {
                let why = AcquireWhy::Failed(error.to_string());
                self.abandon(&grant, AcquirePhase::Capture, began, why, out)
                    .await;
            }
            Bound::TimedOut => {
                let why = AcquireWhy::TimedOut;
                self.abandon(&grant, AcquirePhase::Capture, began, why, out)
                    .await;
            }
        }
    }

    /// A step of taking the window failed: the log says which, the lease is given back (the
    /// input too, once it was attached) and the run is told the step by name.
    async fn abandon(
        &mut self,
        grant: &LeaseGrant,
        phase: AcquirePhase,
        began: Stamp,
        why: AcquireWhy,
        out: &mut impl Outlet,
    ) {
        let failed = Progress::Failed(self.since(began), why.clone());
        self.note(out, Step::Phase(phase), failed);
        let _ = self.seam.seat.detach(grant.id).await;
        let _ = self.seam.desktop.release(grant.id).await;
        out.input(RunInput::AcquireFailed(AcquireFailure { phase, why }));
    }

    fn since(&self, began: Stamp) -> u64 {
        self.seam.clock.now().0.saturating_sub(began.0)
    }

    fn note(&self, out: &mut impl Outlet, step: Step, progress: Progress) {
        out.note(StepNote {
            run: self.run.clone(),
            step,
            progress,
        });
    }

    /// Points the capture at `front`, the window in front of the lease: the leased window's own
    /// session, or a session opened on the dialog it opened (ended once the dialog is not in
    /// front any more).
    pub(super) async fn follow(&mut self, front: &ToplevelInfo) -> Result<Following, CaptureError> {
        let held = self.held.as_mut().ok_or(CaptureError::SourceGone)?;
        if front.ident == held.grant.window.ident {
            held.unfollow();
            return Ok(Following::Window);
        }
        if held.dialog.as_ref().is_some_and(|d| d.ident == front.ident) {
            return Ok(Following::Dialog);
        }
        let grant = LeaseGrant {
            window: front.clone(),
            ..held.grant.clone()
        };
        let capture =
            match bounded(self.seam.clock, PHASE_LIMIT, self.seam.capture.open(&grant)).await {
                Bound::Done(opened) => opened?,
                Bound::TimedOut => {
                    return Err(CaptureError::Protocol(
                        "the dialog's capture did not open in time".to_owned(),
                    ));
                }
            };
        held.dialog = Some(Dialog {
            ident: front.ident.clone(),
            capture,
        });
        Ok(Following::Dialog)
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
            // A paused run withdraws its question: the question is withdrawn.
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
        // The window is read again: the aim was made against an earlier look.
        let Some(now) = self.window_now().await else {
            return out.input(RunInput::InjectFailed(SeatError::TargetGone));
        };
        let wants = takes(&planned.action);
        let (now, refocus) = self.refocused(now, planned.window, wants).await;
        let now = match self.settled(now, planned.window, wants).await {
            (now, Fit::Ready) => now,
            (_, Fit::NotVisible) => {
                return out.input(RunInput::InjectFailed(SeatError::OutsideLease));
            }
            // Resized under the aim: the run looks again.
            (_, Fit::Resized) => return out.input(RunInput::StateMoved(Moved::Geometry)),
            // Keys for a window that does not have the focus, and it did not come: the model is
            // told, and a click in the window gives it the focus.
            (_, Fit::NotFocused) => {
                return out.input(RunInput::InjectFailed(SeatError::NotFocused));
            }
        };
        // Only the app the person allowed is driven: the same window id under another app is
        // not the window that was leased.
        if now.app != self.task.app {
            return out.input(RunInput::StateMoved(Moved::Identity));
        }
        // Not what the aim was made against (moved, another owner, the focus gone elsewhere):
        // nothing is injected and the run looks again.
        let seen = self.seen.clone().map(|seen| seen.after(refocus));
        if let Some(moved) = seen.as_ref().and_then(|seen| seen.moved_by(&now)) {
            return out.input(RunInput::StateMoved(moved));
        }
        if let Err(error) = self.seam.seat.rebind(lease, now.geometry).await {
            return out.input(RunInput::InjectFailed(error));
        }
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
        if !self.model_open {
            let begin = CuaBegin::new(
                crate::briefing::task_line(&self.task),
                porter_capability::CuaEnv::Desktop,
            )
            .with_hints(self.hints.clone());
            if let Err(error) = model.begin(begin).await {
                return out.input(failed_input(&*model, error));
            }
            self.model_open = true;
        }
        let reply = model.step(request, frame, &mut Forward(out)).await;
        if let Some(served) = model.served()
            && self.announced.as_ref() != Some(&served)
        {
            self.announced = Some(served.clone());
            out.input(RunInput::Routed(served));
        }
        out.input(match reply {
            Ok(reply) => {
                let usage = model.usage();
                let cloud = model
                    .served()
                    .is_some_and(|s| matches!(s.locality, Locality::Cloud { .. }));
                RunInput::ModelReplied(Box::new(ModelReply {
                    reply,
                    usage,
                    charge: usage.charge,
                    cloud,
                }))
            }
            Err(error) => failed_input(&*model, error),
        });
    }
}

/// What the machine is told when the model's call failed with `error`: a model that cannot read
/// images says so in its own words.
fn failed_input(model: &impl CuaModel, error: CuaModelError) -> RunInput {
    match error {
        CuaModelError::CannotSeeScreen => RunInput::ModelBlind(model.cannot_see_words()),
        other => RunInput::ModelFailed(other),
    }
}
