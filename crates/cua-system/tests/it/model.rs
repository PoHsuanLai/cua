//! `InferdCuaModel` over a scripted inferd session: what it opens, what it sends (the frame as a
//! sealed memfd at `AttachIndex(0)`, the layout from the buffer), what it forwards to the sink,
//! and how every way a turn can end is sorted. Nothing here touches a bus, a GPU or a real inferd.

use crate::scripted::*;
use cua_action::{DeviceSize, PixelFormat};
use cua_run::{CuaModel, CuaModelError, ModelReach, StepEvent};
use cua_system::InferdCuaModel;
use porter_core::capability::CuaEnv;
use porter_core::{DataClass, Need, Tier};
use porter_infer::{
    AttachIndex, ClientFrame, CuaStepReply, FrameLayout, ImageSource, InferEvent, InferRefusal,
    InferReply, InferRequest, ModelError, Readiness,
};
use rustix::fs::SealFlags;
use std::collections::BTreeSet;

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
            "no model sees the screen",
            refused(InferRefusal::NoModelSeesScreen),
            CuaModelError::CannotSeeScreen,
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
