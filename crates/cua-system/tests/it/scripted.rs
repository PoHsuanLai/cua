//! A scripted inferd for the model tests: a transport whose sessions answer from a list of
//! batches of events, and the log of what they were sent.

use std::io::Read;

use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, PixelFormat, Point, Scale120, Size, Target,
    WindowSpace,
};
use cua_run::{FrameAttachment, StepEvent, StepSink};
use maskcap::PixelBuf;
use porter_capability::CuaEnv;
use porter_capability::{Locality, ModelId, Tier};
use porter_client::{Transport, TransportError};
use porter_core::{AccountId, DataClass};
use porter_ids::Tokens;
use porter_infer::{
    AttachIndex, ClientFrame, CuaBegin, CuaStepReply, CuaStepRequest, FrameImage, FrameLayout,
    ImageSource, InferEvent, InferReply, InferSession, MaskedRegions, NoteFrom, OpenOptions,
    PlaceRow, Readiness, ServedBy, SessionError, StepIndex, StepNote, TokenUsage, TreeText,
    WindowGeometry,
};
use porter_match::Need;
use porter_wire::{AccountsReply, AccountsRequest};
use rustix::fs::{SealFlags, fcntl_get_seals};
use std::collections::{BTreeSet, VecDeque};
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};

/// What the scripted inferd saw.
#[derive(Debug, Default)]
pub(crate) struct Log {
    pub(crate) opened: Vec<(Need, DataClass, Tier)>,
    pub(crate) frames: Vec<ClientFrame>,
    /// Per frame: the contents of each descriptor read from where the receiver finds the offset,
    /// and its seals.
    pub(crate) attachments: Vec<Vec<(Vec<u8>, SealFlags)>>,
}

/// One session whose turns are scripted: each request received releases the next batch of events.
#[derive(Debug)]
pub(crate) struct Scripted {
    turns: Mutex<VecDeque<Vec<InferEvent>>>,
    log: Arc<Mutex<Log>>,
    down: bool,
    /// inferd refuses the caller, with this text.
    denied: Option<String>,
    /// What `Prepare` answers; none is an inferd that does not.
    readiness: Option<Readiness>,
    /// What `Places` answers; none is an inferd that does not.
    places: Option<Vec<PlaceRow>>,
}

pub(crate) struct Session {
    turns: VecDeque<Vec<InferEvent>>,
    ready: VecDeque<InferEvent>,
    log: Arc<Mutex<Log>>,
}

impl Scripted {
    pub(crate) fn new(turns: Vec<Vec<InferEvent>>) -> (Self, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        let scripted = Self {
            turns: Mutex::new(turns.into()),
            log: log.clone(),
            down: false,
            denied: None,
            readiness: None,
            places: None,
        };
        (scripted, log)
    }

    pub(crate) fn unreachable() -> Self {
        Self {
            turns: Mutex::default(),
            log: Arc::default(),
            down: true,
            denied: None,
            readiness: None,
            places: None,
        }
    }

    pub(crate) fn denying(why: &str) -> Self {
        Self {
            turns: Mutex::default(),
            log: Arc::default(),
            down: false,
            denied: Some(why.to_owned()),
            readiness: None,
            places: None,
        }
    }

    pub(crate) fn placing(places: Vec<PlaceRow>) -> Self {
        Self {
            places: Some(places),
            ..Self::unreachable()
        }
    }

    pub(crate) fn preparing(readiness: Readiness) -> Self {
        Self {
            readiness: Some(readiness),
            ..Self::unreachable()
        }
    }
}

impl InferSession for Session {
    async fn send(&mut self, frame: ClientFrame) -> Result<(), SessionError> {
        self.send_attached(frame, vec![]).await
    }

    async fn send_attached(
        &mut self,
        frame: ClientFrame,
        attachments: Vec<OwnedFd>,
    ) -> Result<(), SessionError> {
        let seen = attachments
            .into_iter()
            .map(|fd| {
                let seals = fcntl_get_seals(&fd).expect("seals");
                let mut bytes = Vec::new();
                std::fs::File::from(fd)
                    .read_to_end(&mut bytes)
                    .expect("read the memfd");
                (bytes, seals)
            })
            .collect();
        let mut log = self.log.lock().expect("lock");
        log.frames.push(frame.clone());
        log.attachments.push(seen);
        if matches!(frame, ClientFrame::Request(_)) {
            self.ready
                .extend(self.turns.pop_front().ok_or(SessionError::Closed)?);
        }
        Ok(())
    }

    async fn next(&mut self) -> Result<InferEvent, SessionError> {
        self.ready.pop_front().ok_or(SessionError::Closed)
    }
}

impl Transport for Scripted {
    type Session = Session;

    async fn call(&self, _request: AccountsRequest) -> Result<AccountsReply, TransportError> {
        Err(TransportError::Unreachable)
    }

    async fn prepare(
        &self,
        _need: &Need,
        _class: DataClass,
        _tier: Tier,
        _options: &OpenOptions,
    ) -> Result<Readiness, TransportError> {
        self.readiness.ok_or(TransportError::Unreachable)
    }

    async fn places(&self) -> Result<Vec<PlaceRow>, TransportError> {
        self.places.clone().ok_or(TransportError::Unreachable)
    }

    async fn open_with(
        &self,
        need: &Need,
        class: DataClass,
        tier: Tier,
        _options: &OpenOptions,
    ) -> Result<Session, TransportError> {
        if self.down {
            return Err(TransportError::Unreachable);
        }
        if let Some(why) = &self.denied {
            return Err(TransportError::Denied(why.clone()));
        }
        self.log
            .lock()
            .expect("lock")
            .opened
            .push((need.clone(), class, tier));
        Ok(Session {
            turns: std::mem::take(&mut *self.turns.lock().expect("lock")),
            ready: VecDeque::new(),
            log: self.log.clone(),
        })
    }
}

#[derive(Debug, Default)]
pub(crate) struct Collect(pub(crate) Vec<StepEvent>);

impl StepSink for Collect {
    fn event(&mut self, event: StepEvent) {
        self.0.push(event);
    }
}

pub(crate) fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("qwen-vl").expect("model"),
        locality: Locality::OnDevice,
    }
}

pub(crate) fn empty_reply() -> CuaStepReply {
    CuaStepReply::new(vec![])
}

pub(crate) fn click() -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: Target::Point(Point::new(Coord(3), Coord(4))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

pub(crate) fn begin() -> CuaBegin {
    CuaBegin::new("archive the thread".into(), CuaEnv::Desktop).with_hints(vec!["mail".into()])
}

pub(crate) fn ack() -> Vec<InferEvent> {
    vec![
        InferEvent::Routed(served()),
        InferEvent::Finished(InferReply::CuaStep(empty_reply())),
    ]
}

pub(crate) fn usage(n: u32) -> TokenUsage {
    TokenUsage {
        input: Tokens(n),
        output: Tokens(1),
        cached: Tokens(0),
        charge: None,
    }
}

pub(crate) fn frame_bytes() -> Vec<u8> {
    (0..16).collect()
}

pub(crate) fn attachment() -> FrameAttachment {
    FrameAttachment(PixelBuf {
        format: PixelFormat::Xbgr8888,
        size: DeviceSize { w: 2, h: 2 },
        stride: 8,
        data: Arc::from(frame_bytes()),
    })
}

pub(crate) fn request(note: &str) -> CuaStepRequest {
    CuaStepRequest::new(
        StepIndex(1),
        WindowGeometry {
            logical: Size::new(Coord(2), Coord(2)),
            scale: Scale120(120),
        },
        // Deliberately wrong: the model attaches the buffer it is given, whatever turn said.
        FrameImage {
            source: ImageSource::Attached(AttachIndex(7)),
            layout: FrameLayout::Raw {
                format: PixelFormat::Abgr8888,
                size: DeviceSize { w: 9, h: 9 },
                stride: 36,
            },
        },
        TreeText::Absent,
    )
    .with_masked(MaskedRegions(0))
    .with_notes(vec![StepNote {
        from: NoteFrom::Person,
        text: note.into(),
    }])
}
