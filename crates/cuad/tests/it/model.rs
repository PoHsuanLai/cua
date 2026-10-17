//! `InferdCuaModel` over a scripted inferd session: what it opens, what it sends (the frame as a
//! sealed memfd at `AttachIndex(0)`, the layout from the buffer), what it forwards to the sink,
//! and how every way a turn can end is sorted. Nothing here touches a bus, a GPU or a real inferd.

use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, PixelFormat, Point, Scale120, Size, Target,
    WindowSpace,
};
use cua_run::{CuaModel, CuaModelError, FrameAttachment, ModelReach, StepEvent, StepSink};
use cuad::InferdCuaModel;
use maskcap::PixelBuf;
use porter_client::{Transport, TransportError};
use porter_core::capability::CuaEnv;
use porter_core::{
    AccountId, AccountsReply, AccountsRequest, DataClass, Locality, ModelId, Need, Tier, Tokens,
};
use porter_infer::{
    AttachIndex, ClientFrame, CuaBegin, CuaStepReply, CuaStepRequest, FrameImage, FrameLayout,
    ImageSource, InferEvent, InferRefusal, InferReply, InferRequest, InferSession, MaskedRegions,
    ModelError, NoteFrom, OpenOptions, Readiness, ServedBy, SessionError, StepIndex, StepNote,
    TokenUsage, TreeText, WindowGeometry,
};
use rustix::fs::{SealFlags, fcntl_get_seals};
use std::collections::{BTreeSet, VecDeque};
use std::io::Read;
use std::os::fd::OwnedFd;
use std::sync::{Arc, Mutex};

/// What the scripted inferd saw.
#[derive(Debug, Default)]
struct Log {
    opened: Vec<(Need, DataClass, Tier)>,
    frames: Vec<ClientFrame>,
    /// Per frame: the contents of each descriptor read from where the receiver finds the offset,
    /// and its seals.
    attachments: Vec<Vec<(Vec<u8>, SealFlags)>>,
}

/// One session whose turns are scripted: each request received releases the next batch of events.
#[derive(Debug)]
struct Scripted {
    turns: Mutex<VecDeque<Vec<InferEvent>>>,
    log: Arc<Mutex<Log>>,
    down: bool,
    /// inferd refuses the caller, with this text.
    denied: Option<String>,
    /// What `Prepare` answers; none is an inferd that does not.
    readiness: Option<Readiness>,
}

struct Session {
    turns: VecDeque<Vec<InferEvent>>,
    ready: VecDeque<InferEvent>,
    log: Arc<Mutex<Log>>,
}

impl Scripted {
    fn new(turns: Vec<Vec<InferEvent>>) -> (Self, Arc<Mutex<Log>>) {
        let log = Arc::new(Mutex::new(Log::default()));
        let scripted = Self {
            turns: Mutex::new(turns.into()),
            log: log.clone(),
            down: false,
            denied: None,
            readiness: None,
        };
        (scripted, log)
    }

    fn unreachable() -> Self {
        Self {
            turns: Mutex::default(),
            log: Arc::default(),
            down: true,
            denied: None,
            readiness: None,
        }
    }

    fn denying(why: &str) -> Self {
        Self {
            turns: Mutex::default(),
            log: Arc::default(),
            down: false,
            denied: Some(why.to_owned()),
            readiness: None,
        }
    }

    fn preparing(readiness: Readiness) -> Self {
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
struct Collect(Vec<StepEvent>);

impl StepSink for Collect {
    fn event(&mut self, event: StepEvent) {
        self.0.push(event);
    }
}

fn served() -> ServedBy {
    ServedBy {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse("qwen-vl").expect("model"),
        locality: Locality::OnDevice,
    }
}

fn empty_reply() -> CuaStepReply {
    CuaStepReply::new(vec![])
}

fn click() -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: Target::Point(Point::new(Coord(3), Coord(4))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

fn begin() -> CuaBegin {
    CuaBegin::new("archive the thread".into(), CuaEnv::Desktop).with_hints(vec!["mail".into()])
}

fn ack() -> Vec<InferEvent> {
    vec![
        InferEvent::Routed(served()),
        InferEvent::Finished(InferReply::CuaStep(empty_reply())),
    ]
}

fn usage(n: u32) -> TokenUsage {
    TokenUsage {
        input: Tokens(n),
        output: Tokens(1),
        cached: Tokens(0),
    }
}

fn frame_bytes() -> Vec<u8> {
    (0..16).collect()
}

fn attachment() -> FrameAttachment {
    FrameAttachment(PixelBuf {
        format: PixelFormat::Xbgr8888,
        size: DeviceSize { w: 2, h: 2 },
        stride: 8,
        data: Arc::from(frame_bytes()),
    })
}

fn request(note: &str) -> CuaStepRequest {
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

#[tokio::test]
async fn begin_opens_a_computer_use_screen_session_and_reads_to_the_acknowledgement() {
    let (transport, log) = Scripted::new(vec![ack()]);
    let mut model = InferdCuaModel::over(transport).on_tier(Tier::Best);
    assert_eq!(model.served(), None);
    model.begin(begin()).await.expect("begun");

    let log = log.lock().expect("lock");
    let [(need, class, tier)] = log.opened.as_slice() else {
        panic!("one session: {:?}", log.opened)
    };
    let Need::ComputerUse(need) = need else {
        panic!("{need:?}")
    };
    assert_eq!(need.environments, BTreeSet::from([CuaEnv::Desktop]));
    assert_eq!((*class, *tier), (DataClass::Screen, Tier::Best));
    assert_eq!(
        log.frames,
        vec![ClientFrame::Request(InferRequest::CuaBegin(begin()))]
    );
    assert_eq!(log.attachments, vec![vec![]], "a begin carries no frame");
    drop(log);
    assert_eq!(model.served(), Some(served()), "the route was read");
}

#[tokio::test]
async fn a_step_sends_the_frame_as_a_sealed_memfd_and_forwards_what_streams() {
    let reply = CuaStepReply::new(vec![click()]).with_thought("the button is at 3,4".into());
    let (transport, log) = Scripted::new(vec![
        ack(),
        vec![
            InferEvent::Waiting(porter_infer::Readiness::Loading),
            InferEvent::ThoughtDelta("the button ".into()),
            InferEvent::Usage(usage(10)),
            InferEvent::ThoughtDelta("is at 3,4".into()),
            InferEvent::ActionProposed(click()),
            InferEvent::Usage(usage(20)),
            InferEvent::Finished(InferReply::CuaStep(reply.clone())),
        ],
    ]);
    let mut model = InferdCuaModel::over(transport);
    model.begin(begin()).await.expect("begun");
    let mut sink = Collect::default();
    let got = model
        .step(request("use the blue one"), attachment(), &mut sink)
        .await
        .expect("a reply");
    assert_eq!(got, reply);

    // The sink saw the thoughts and the proposal, in order; nothing else.
    assert_eq!(
        sink.0,
        vec![
            StepEvent::ThoughtDelta(cua_run::ThoughtText("the button ".into())),
            StepEvent::ThoughtDelta(cua_run::ThoughtText("is at 3,4".into())),
            StepEvent::ActionProposed(click()),
        ]
    );
    assert_eq!(model.usage(), usage(20), "the last usage of the step");

    let log = log.lock().expect("lock");
    let ClientFrame::Request(InferRequest::CuaStep(sent)) = &log.frames[1] else {
        panic!("{:?}", log.frames[1])
    };
    assert_eq!(sent.frame.source, ImageSource::Attached(AttachIndex(0)));
    assert_eq!(
        sent.frame.layout,
        FrameLayout::Raw {
            format: PixelFormat::Xbgr8888,
            size: DeviceSize { w: 2, h: 2 },
            stride: 8,
        },
        "the layout is the buffer's"
    );
    assert_eq!(sent.notes.len(), 1, "the run's notes ride along");
    assert_eq!(sent.notes[0].text, "use the blue one");
    let [(bytes, seals)] = log.attachments[1].as_slice() else {
        panic!("one descriptor: {:?}", log.attachments[1].len())
    };
    assert_eq!(bytes, &frame_bytes(), "read from the start, whole");
    assert!(seals.contains(SealFlags::WRITE | SealFlags::SHRINK | SealFlags::GROW));
}

#[tokio::test]
async fn every_way_a_turn_can_end_is_sorted_for_the_run_machine() {
    let failed = |e| vec![InferEvent::Finished(InferReply::Failed(e))];
    let refused = |r| vec![InferEvent::Finished(InferReply::Refused(r))];
    let cases: Vec<(&str, Vec<InferEvent>, CuaModelError)> = vec![
        (
            "engine away",
            failed(ModelError::Unreachable),
            CuaModelError::Transient,
        ),
        (
            "engine loading",
            failed(ModelError::NotReady),
            CuaModelError::Transient,
        ),
        (
            "rate limited",
            failed(ModelError::RateLimited(3)),
            CuaModelError::Transient,
        ),
        (
            "unreadable",
            failed(ModelError::Unreadable),
            CuaModelError::Parse,
        ),
        (
            "unparseable",
            failed(ModelError::Unparseable),
            CuaModelError::Parse,
        ),
        (
            "model refused",
            failed(ModelError::Refused),
            CuaModelError::Fatal,
        ),
        (
            "overflow",
            failed(ModelError::ContextOverflow),
            CuaModelError::Fatal,
        ),
        (
            "no model",
            refused(InferRefusal::Unavailable),
            CuaModelError::Transient,
        ),
        (
            "no grant",
            refused(InferRefusal::NeedsGrant),
            CuaModelError::Fatal,
        ),
        (
            "over budget",
            refused(InferRefusal::OverBudget),
            CuaModelError::Fatal,
        ),
        (
            "cancelled",
            vec![InferEvent::Finished(InferReply::Cancelled)],
            CuaModelError::Transient,
        ),
        (
            "another kind of reply",
            vec![InferEvent::Finished(InferReply::Embed(
                porter_infer::EmbedReply::new(vec![], usage(1), served()),
            ))],
            CuaModelError::Fatal,
        ),
    ];
    for (name, turn, want) in cases {
        let (transport, _) = Scripted::new(vec![ack(), turn]);
        let mut model = InferdCuaModel::over(transport);
        model.begin(begin()).await.expect("begun");
        let got = model
            .step(request("x"), attachment(), &mut Collect::default())
            .await;
        assert_eq!(got, Err(want), "{name}");
    }
}

#[tokio::test]
async fn a_model_failure_leaves_the_session_usable_and_a_closed_one_does_not() {
    let (transport, _) = Scripted::new(vec![
        ack(),
        vec![InferEvent::Finished(InferReply::Failed(
            ModelError::NotReady,
        ))],
        vec![InferEvent::Finished(InferReply::CuaStep(empty_reply()))],
        // The session ends in the middle of this turn: no Finished.
        vec![InferEvent::ThoughtDelta("half".into())],
    ]);
    let mut model = InferdCuaModel::over(transport);
    model.begin(begin()).await.expect("begun");
    async fn step(model: &mut InferdCuaModel<Scripted>) -> Result<CuaStepReply, CuaModelError> {
        model
            .step(request("x"), attachment(), &mut Collect::default())
            .await
    }
    assert_eq!(step(&mut model).await, Err(CuaModelError::Transient));
    assert_eq!(
        step(&mut model).await,
        Ok(empty_reply()),
        "the retry is the same session"
    );
    assert_eq!(
        step(&mut model).await,
        Err(CuaModelError::Fatal),
        "the session closed mid-turn"
    );
    assert_eq!(
        step(&mut model).await,
        Err(CuaModelError::Fatal),
        "and the history is gone"
    );
}

#[tokio::test]
async fn only_a_model_that_is_plainly_not_there_is_missing_before_a_run_starts() {
    let rows = [
        (Some(Readiness::Unavailable), ModelReach::Missing),
        (Some(Readiness::Ready), ModelReach::Serves),
        (Some(Readiness::Loadable), ModelReach::Serves),
        (Some(Readiness::Downloadable), ModelReach::Serves),
        // An inferd that does not answer is the run's to find out.
        (None, ModelReach::Serves),
    ];
    for (readiness, want) in rows {
        let transport = match readiness {
            Some(readiness) => Scripted::preparing(readiness),
            None => Scripted::unreachable(),
        };
        assert_eq!(
            InferdCuaModel::over(transport).reach().await,
            want,
            "{readiness:?}"
        );
    }
}

#[tokio::test]
async fn faults_before_and_around_the_session() {
    // inferd away: worth retrying. A step before a begin: nothing to step.
    let mut down = InferdCuaModel::over(Scripted::unreachable());
    assert_eq!(down.begin(begin()).await, Err(CuaModelError::Transient));
    assert_eq!(
        down.step(request("x"), attachment(), &mut Collect::default())
            .await,
        Err(CuaModelError::Fatal)
    );
    // inferd refusing the caller (its table does not name cuad) is not worth retrying.
    assert_eq!(
        InferdCuaModel::over(Scripted::denying("the caller is not in its caller table"))
            .begin(begin())
            .await,
        Err(CuaModelError::Fatal)
    );
    // A begin answered with a step (actions in the acknowledgement) is a protocol fault.
    let acting = CuaStepReply::new(vec![click()]);
    let (transport, _) = Scripted::new(vec![vec![InferEvent::Finished(InferReply::CuaStep(
        acting,
    ))]]);
    assert_eq!(
        InferdCuaModel::over(transport).begin(begin()).await,
        Err(CuaModelError::Fatal)
    );
    // A refused begin says why it ended.
    let (transport, _) = Scripted::new(vec![vec![InferEvent::Finished(InferReply::Refused(
        InferRefusal::RequiresCloud(DataClass::Screen),
    ))]]);
    assert_eq!(
        InferdCuaModel::over(transport).begin(begin()).await,
        Err(CuaModelError::Fatal)
    );
}
