//! The seams of a run over a stage: a seat that acts on the surface, a capture that draws it, a
//! desktop that names its window, and the clock they share.

use crate::keys;
use crate::stage::{ScreenChange, Stage};
use cua_action::{Coord, DeviceSize, Point, Rect, Scale120, Size};
use cua_desktop::{
    BackendCaps, BackendKind, Desktop, DesktopError, DesktopEvent, EiSocket, GlowState,
    LeaseControl, LeaseGrant, LeaseId, LeaseRequest, Leased, SeatIsolation, Stamp, Suspend,
    ToplevelIdent, ToplevelInfo,
};
use cua_fake::{FakeA11y, FakeDesktop, MemoryEvents};
use cua_run::{Clock, CuaModel, Seams};
use maskcap::{
    CaptureError, CaptureSession, DamageAt, DamageSource, Frame, FrameSeq, FrameWant, MaskRegion,
    MaskReport, WholeMask, WindowCapture,
};
use porter_core::{AppName, Count, Px, UnixSeconds};
use seat_input::{
    AgentSeat, Emulation, InputOp, InputOrigin, InputReceipt, Keymap, SeatCaps, SeatError,
};
use std::marker::PhantomData;
use std::sync::{Arc, Mutex};

/// The clock of an eval run: it moves only when the driver says time passes.
#[derive(Debug, Clone)]
pub struct EvalClock(pub Arc<cua_fake::FixedClock>);

impl Clock for EvalClock {
    fn now(&self) -> Stamp {
        self.0.now()
    }

    fn unix(&self) -> UnixSeconds {
        self.0.unix()
    }
}

/// The seams of an eval run, for a model `M`.
#[derive(Debug)]
pub struct EvalSeams<M>(PhantomData<fn() -> M>);

impl<M: CuaModel + 'static> Seams for EvalSeams<M> {
    type Desktop = EvalDesktop;
    type Seat = EvalSeat;
    type Capture = EvalCapture;
    type A11y = FakeA11y;
    type Model = M;
    type Gate = crate::gate::EvalGate;
    type Events = MemoryEvents;
    type Time = EvalClock;
}

/// The agent's hands, acting on the stage.
#[derive(Debug)]
pub struct EvalSeat {
    stage: Arc<Stage>,
    clock: EvalClock,
    keymap: Arc<Keymap>,
    emulation: Mutex<Emulation>,
}

impl EvalSeat {
    /// A seat on `stage`.
    pub fn new(stage: Arc<Stage>, clock: EvalClock) -> EvalSeat {
        EvalSeat {
            stage,
            clock,
            keymap: Arc::new(keys::keymap()),
            emulation: Mutex::new(Emulation::Running),
        }
    }
}

impl AgentSeat for EvalSeat {
    fn caps(&self) -> SeatCaps {
        SeatCaps::for_isolation(SeatIsolation::SharedSeat)
    }

    fn keymap(&self) -> Arc<Keymap> {
        self.keymap.clone()
    }

    async fn attach(&self, _lease: &LeaseGrant, socket: EiSocket) -> Result<(), SeatError> {
        drop(socket);
        Ok(())
    }

    async fn perform(&self, lease: LeaseId, ops: &[InputOp]) -> Result<InputReceipt, SeatError> {
        if *self.emulation.lock().expect("emulation lock") == Emulation::Stopped {
            return Err(SeatError::Suspended);
        }
        self.stage.apply(ops);
        Ok(InputReceipt {
            lease,
            ops: Count(u32::try_from(ops.len()).unwrap_or(u32::MAX)),
            text_path: None,
            origin: InputOrigin::SharedSeatViaEi,
            at: self.clock.now(),
        })
    }

    async fn emulation(&self, _lease: LeaseId, state: Emulation) -> Result<(), SeatError> {
        *self.emulation.lock().expect("emulation lock") = state;
        Ok(())
    }

    async fn detach(&self, _lease: LeaseId) -> Result<(), SeatError> {
        Ok(())
    }
}

/// Capture of the stage's window.
#[derive(Debug)]
pub struct EvalCapture {
    stage: Arc<Stage>,
    clock: EvalClock,
}

impl EvalCapture {
    /// A capture of `stage`.
    pub fn new(stage: Arc<Stage>, clock: EvalClock) -> EvalCapture {
        EvalCapture { stage, clock }
    }
}

/// One open capture session.
#[derive(Debug)]
pub struct EvalCaptureSession {
    stage: Arc<Stage>,
    clock: EvalClock,
    seq: u64,
}

impl WindowCapture for EvalCapture {
    type Session = EvalCaptureSession;

    async fn open(&self, _lease: &LeaseGrant) -> Result<EvalCaptureSession, CaptureError> {
        Ok(EvalCaptureSession {
            stage: self.stage.clone(),
            clock: self.clock.clone(),
            seq: 0,
        })
    }
}

impl CaptureSession for EvalCaptureSession {
    async fn frame(
        &mut self,
        _want: FrameWant,
        _hints: &[MaskRegion],
    ) -> Result<Frame, CaptureError> {
        self.seq += 1;
        let pixels = self.stage.draw();
        let DeviceSize { w, h } = pixels.size;
        Ok(Frame {
            seq: FrameSeq(self.seq),
            at: self.clock.now(),
            pixels,
            logical: Size::new(Coord(w), Coord(h)),
            scale: Scale120(120),
            damage: vec![],
            masks: MaskReport {
                applied_by: cua_desktop::MaskWhere::Cuad,
                whole: WholeMask::None,
                regions: vec![],
            },
        })
    }

    async fn damage(&mut self) -> Result<DamageAt, CaptureError> {
        match self.stage.changed() {
            ScreenChange::Same => Err(CaptureError::SourceGone),
            ScreenChange::Changed([w, h]) => Ok(DamageAt {
                at: self.clock.now(),
                region: Rect {
                    origin: Point::new(Coord(0), Coord(0)),
                    size: Size::new(Coord(w), Coord(h)),
                },
                source: DamageSource::PixelDiff,
            }),
        }
    }
}

/// A desktop of one window, whose title and app follow the stage.
#[derive(Debug)]
pub struct EvalDesktop {
    inner: FakeDesktop,
    stage: Arc<Stage>,
    ident: ToplevelIdent,
}

/// The id of the one window.
pub const WINDOW: &str = "w1";

impl EvalDesktop {
    /// A desktop on stock cosmic-comp's capabilities with the stage's window.
    pub fn new(stage: Arc<Stage>) -> Result<EvalDesktop, DesktopError> {
        let ident =
            ToplevelIdent::parse(WINDOW).map_err(|e| DesktopError::Protocol(e.to_string()))?;
        let window = window_of(&stage, &ident)?;
        Ok(EvalDesktop {
            inner: FakeDesktop::new(BackendKind::CosmicStock, vec![window]),
            stage,
            ident,
        })
    }

    /// The window's identifier.
    pub fn ident(&self) -> &ToplevelIdent {
        &self.ident
    }
}

fn window_of(stage: &Stage, ident: &ToplevelIdent) -> Result<ToplevelInfo, DesktopError> {
    let now = stage.window();
    let app = AppName::parse(&now.app).map_err(|e| DesktopError::Protocol(e.to_string()))?;
    Ok(ToplevelInfo {
        ident: ident.clone(),
        app,
        title: cua_desktop::WindowTitle(now.title),
        pid: cua_desktop::Pid(4242),
        geometry: cua_desktop::GlobalRect {
            x: cua_desktop::GlobalCoord(0),
            y: cua_desktop::GlobalCoord(0),
            w: Px(now.size[0]),
            h: Px(now.size[1]),
        },
        workspace: cua_desktop::WorkspaceRef {
            id: "ws-1".into(),
            shown: cua_desktop::WorkspaceShown::Shown,
        },
        state: std::collections::BTreeSet::new(),
        trust: docket_core::WindowTrust::Native,
    })
}

impl Desktop for EvalDesktop {
    fn caps(&self) -> BackendCaps {
        self.inner.caps()
    }

    async fn windows(&self) -> Result<Vec<ToplevelInfo>, DesktopError> {
        Ok(vec![window_of(&self.stage, &self.ident)?])
    }

    async fn next_event(&self) -> DesktopEvent {
        self.inner.next_event().await
    }
}

impl LeaseControl for EvalDesktop {
    async fn lease(&self, req: LeaseRequest) -> Result<Leased, DesktopError> {
        self.inner.lease(req).await
    }

    async fn release(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.inner.release(lease).await
    }

    async fn suspend(&self, lease: LeaseId, why: Suspend) -> Result<(), DesktopError> {
        self.inner.suspend(lease, why).await
    }

    async fn resume(&self, lease: LeaseId) -> Result<(), DesktopError> {
        self.inner.resume(lease).await
    }

    async fn glow(&self, lease: LeaseId, glow: GlowState) -> Result<(), DesktopError> {
        self.inner.glow(lease, glow).await
    }
}
