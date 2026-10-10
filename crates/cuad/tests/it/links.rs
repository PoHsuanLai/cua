//! cuad's seams over the real daemons: the router's gate and message path through docket-client's
//! `DbusTransport` against a real intentd (over docket's fakes) on a private bus, and the event
//! log through almanac-client against a real memory service (over almanac's fakes). Each caller is
//! a connection that owns the well-known name its role is configured to have.

use crate::support::bus::PrivateBus;
use almanac_client::{InProcess, Memory};
use almanac_core::{
    ActorFilter, Caller, Count, SpaceId as MemSpace, TimelineFilter, TimelineQuery, TrustFilter,
};
use cua_action::{CuaAction, WindowSpace};
use cua_action::{RunMode, WindowTrust};
use cua_bus::{
    ActionResult, AuthorisedBy, CuaOutcome, CuaRecord, FrameNote, TargetedAction, ThoughtDigest,
    WindowRef,
};
use cua_desktop::Digest32;
use cua_run::{GateCheck, GateEvent, GateLink, MessageLink, RecordSink};
use cuad::{DaemonLinks, IntentsGate};
use docket_client::{DbusTransport, Intents};
use docket_core::{
    CallRefusal, CallerRole, ConfirmAnswer, ConfirmEnd, CuaAsk, DraftPart, EffectBasis, GateAnswer,
    GrantAnswer, GrantAsk, GrantScope, InboundPart, InboxAsk, IntentsRequest, MessageDraft,
    SessionOpen,
};
use docket_fake::{ScriptedConfirmer, fake_router};
use docket_router::Router;
use intentd::{IntentdConfig, serve_on};
use maskcap::FrameDigest;
use porter_core::AppName;
use prov::{
    Address, AgentRef, ConfirmReceipt, Effect, InputProof, Label, MessageKind, MessageText, RunId,
    SessionId, Source, SpaceId, SpaceScope, UnixSeconds,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use zbus::Connection;

const COMPANION: &str = "org.quire.Companion1";
const CUAD: &str = "org.quire.Cua1";

fn mail() -> AppName {
    AppName::parse("org.quire.Mail").unwrap()
}

fn work() -> SpaceId {
    SpaceId::parse("work").unwrap()
}

fn run() -> RunId {
    RunId::parse("r-1").unwrap()
}

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").unwrap(),
            input: InputProof::HardwareSeat,
            at: UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    }
}

/// A real intentd over the fake router, on a private bus.
struct Desk {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    _daemon: Connection,
}

async fn desk(answers: Vec<ConfirmAnswer>) -> Desk {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    let mut router: Router<docket_fake::FakeSeams> =
        fake_router(docket_core::AgentConfig::default()).expect("a router");
    router.seams.confirmer = ScriptedConfirmer::answering(answers);
    let mut roles = BTreeMap::new();
    roles.insert(
        CallerRole::Companion,
        vec![AppName::parse(COMPANION).unwrap()],
    );
    roles.insert(CallerRole::Cua, vec![AppName::parse(CUAD).unwrap()]);
    let config = IntentdConfig {
        roles,
        reviewers: None,
        agent: docket_core::AgentConfig::default(),
    };
    serve_on(&daemon, Arc::new(router), Arc::new(config))
        .await
        .expect("intentd serves");
    Desk {
        _dir: dir,
        bus,
        _daemon: daemon,
    }
}

impl Desk {
    async fn as_companion(&self) -> Intents<DbusTransport> {
        let connection = self.bus.connect().await;
        connection.request_name(COMPANION).await.expect("a name");
        // The connection is kept alive by the transport.
        Intents::over(DbusTransport::new(connection))
    }

    async fn cuad_connection(&self) -> Connection {
        let connection = self.bus.connect().await;
        connection.request_name(CUAD).await.expect("a name");
        connection
    }

    /// What the planner does when it starts a run: opens the run's session, and its own to
    /// hear back on.
    async fn open_sessions(&self, planner: &Intents<DbusTransport>) -> SessionId {
        let hearing = planner
            .session_open(SessionOpen {
                space: work(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                external: None,
                started_from: None,
            })
            .await
            .expect("the planner's session");
        let _ = hearing;
        planner
            .session_open(SessionOpen {
                space: work(),
                agent: AgentRef::Cua { run: run() },
                parent: None,
                cwd: None,
                external: None,
                started_from: None,
            })
            .await
            .expect("the run's session")
            .session
    }
}

fn step(effect: Effect) -> CuaAsk {
    CuaAsk {
        run: run(),
        step: 1,
        app: mail(),
        trust: WindowTrust::Quire,
        mode: RunMode::InPlace,
        space: work(),
        action: CuaAction::<WindowSpace>::Observe,
        node: None,
        effect,
        basis: EffectBasis::DefaultTable,
        screen: Label::untrusted(
            Source::Screen { app: mail() },
            porter_core::DataClass::Mail,
            work(),
        ),
    }
}

async fn answered(gate: &IntentsGate<DbusTransport>, ask: CuaAsk) -> GateAnswer {
    let mut check = gate.check(ask).await.expect("a check");
    loop {
        match check.next().await.expect("an event") {
            GateEvent::Answered(answer) => return answer,
            // The router is about to ask the person: cuad's input is idle, so say go on.
            GateEvent::Confirming(_) => check.proceed().await.expect("proceed"),
        }
    }
}

#[tokio::test]
async fn the_gate_asks_the_person_once_per_app_and_then_runs_reads_and_asks_again_for_sends() {
    let desk = desk(vec![yes()]).await;
    let planner = desk.as_companion().await;
    desk.open_sessions(&planner).await;
    let gate = IntentsGate::over(DbusTransport::new(desk.cuad_connection().await));

    assert_eq!(
        gate.grant(GrantAsk {
            app: mail(),
            space: work()
        })
        .await,
        Ok(GrantAnswer::Granted),
        "the person said yes"
    );
    assert_eq!(answered(&gate, step(Effect::Read)).await, GateAnswer::Run);
    let outbound = answered(&gate, step(Effect::Outbound)).await;
    assert!(
        matches!(
            outbound,
            GateAnswer::Refused(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed))
        ),
        "a step that sends asks again, and nobody answered: {outbound:?}"
    );
}

#[tokio::test]
async fn a_refused_consent_and_a_run_nobody_opened_are_the_routers_answers_not_errors() {
    let desk = desk(vec![]).await;
    let gate = IntentsGate::over(DbusTransport::new(desk.cuad_connection().await));
    assert_eq!(
        gate.grant(GrantAsk {
            app: mail(),
            space: work()
        })
        .await,
        Ok(GrantAnswer::Refused(ConfirmEnd::Dismissed))
    );
    assert_eq!(
        answered(&gate, step(Effect::Read)).await,
        GateAnswer::Refused(CallRefusal::Halted(SpaceScope::Any)),
        "no session was opened for r-1, so the router will not rule on its steps"
    );
}

#[tokio::test]
async fn a_check_that_is_dropped_before_its_answer_leaves_the_gate_usable() {
    let desk = desk(vec![yes()]).await;
    let planner = desk.as_companion().await;
    desk.open_sessions(&planner).await;
    let gate = IntentsGate::over(DbusTransport::new(desk.cuad_connection().await));
    gate.grant(GrantAsk {
        app: mail(),
        space: work(),
    })
    .await
    .unwrap();
    drop(gate.check(step(Effect::Read)).await.unwrap());
    assert_eq!(answered(&gate, step(Effect::Read)).await, GateAnswer::Run);
    let mut once = gate.check(step(Effect::Read)).await.unwrap();
    assert!(matches!(once.next().await, Ok(GateEvent::Answered(_))));
    assert!(
        once.next().await.is_err(),
        "an answered check has nothing more to say"
    );
    assert!(once.proceed().await.is_ok());
}

fn links(
    connection: Connection,
    memory: Arc<almanac_service::MemoryService<almanac_fake::FakeBackend>>,
) -> DaemonLinks<InProcess<almanac_fake::FakeBackend>, DbusTransport> {
    DaemonLinks::over(
        Memory::over(InProcess::new(memory, Caller::Cuad)),
        Intents::over(DbusTransport::new(connection)),
    )
    .reading_time_from(|| porter_core::UnixSeconds(1_700_000_000))
}

fn text_of(parts: &[InboundPart]) -> Vec<String> {
    parts
        .iter()
        .map(|p| match p {
            InboundPart::Text(docket_core::Reveal::Plain(t)) => t.clone(),
            other => panic!("not plain text: {other:?}"),
        })
        .collect()
}

#[tokio::test]
async fn reports_go_to_the_requester_and_messages_for_the_run_come_back_through_its_inbox() {
    let desk = desk(vec![]).await;
    let planner = desk.as_companion().await;
    let run_session = desk.open_sessions(&planner).await;
    let service = Arc::new(almanac_fake::fake_service(
        almanac_fake::ScriptedConsolidator::default(),
    ));
    let cuad = links(desk.cuad_connection().await, service);

    // The run reports Done to the planner, in the run's own session.
    let draft = MessageDraft {
        to: Address::new(AgentRef::Companion, work()),
        thread: None,
        in_reply_to: None,
        kind: MessageKind::Report {
            status: prov::ReportStatus::Done,
        },
        parts: vec![DraftPart::Text(MessageText::new("Downloaded the invoice."))],
    };
    let delivery = cuad
        .send(run_session.clone(), draft)
        .await
        .expect("delivered");
    let heard = planner
        .inbox(InboxAsk {
            agent: AgentRef::Companion,
            after: None,
        })
        .await
        .expect("the planner's inbox");
    assert_eq!(heard.len(), 1);
    assert_eq!(heard[0].id, delivery.message);
    assert_eq!(heard[0].from.agent, AgentRef::Cua { run: run() });

    // The planner says something to the run; the run reads it.
    let note = MessageDraft {
        to: Address::new(AgentRef::Cua { run: run() }, work()),
        thread: None,
        in_reply_to: None,
        kind: MessageKind::Note,
        parts: vec![DraftPart::Text(MessageText::new("use the work account"))],
    };
    let planner_session = SessionId::parse("s-1").unwrap();
    planner
        .send(planner_session, note)
        .await
        .expect("sent to the run");
    let lines = cuad
        .inbox(InboxAsk {
            agent: AgentRef::Cua { run: run() },
            after: None,
        })
        .await
        .expect("the run's inbox");
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].from.agent, AgentRef::Companion);
    // What the planner said is its own words, not the screen's: plain or a handle, never lost.
    assert!(
        lines[0]
            .parts
            .iter()
            .all(|p| matches!(p, InboundPart::Text(_))),
        "{:?}",
        lines[0].parts
    );
    let _ = text_of;

    // A session that does not exist is the router's refusal, not a panic.
    let refused = cuad
        .send(
            SessionId::parse("s-99").unwrap(),
            MessageDraft {
                to: Address::new(AgentRef::Companion, work()),
                thread: None,
                in_reply_to: None,
                kind: MessageKind::Note,
                parts: vec![DraftPart::Text(MessageText::new("hello"))],
            },
        )
        .await;
    assert!(
        matches!(refused, Err(cua_run::LinkError::Refused(_))),
        "{refused:?}"
    );
    let _ = IntentsRequest::Manifests;
}

fn record_of_run(run: &RunId) -> Vec<CuaRecord> {
    let action = TargetedAction {
        window: WindowRef(1),
        action: CuaAction::<WindowSpace>::Observe,
    };
    vec![
        CuaRecord::RunStarted {
            run: run.clone(),
            session: SessionId::parse("s-2").unwrap(),
            goal: prov::Labelled::new("sign in".to_owned(), Label::trusted_user()),
            app: mail(),
            space: work(),
            mode: RunMode::InPlace,
            backend: cua_desktop::BackendKind::CosmicStock,
            budget: cua_bus::RunBudget::PROPOSED,
        },
        CuaRecord::Step {
            run: run.clone(),
            n: porter_infer::StepIndex(1),
            thought: ThoughtDigest(Digest32([1; 32])),
            action,
            effect: Effect::Read,
            basis: EffectBasis::DefaultTable,
            authorised_by: AuthorisedBy::Policy,
            result: ActionResult::Done,
            frame: FrameNote::NotKept {
                digest: FrameDigest(Digest32([2; 32])),
                size: cua_action::DeviceSize { w: 10, h: 10 },
            },
            input: None,
        },
        CuaRecord::Finished {
            run: run.clone(),
            outcome: CuaOutcome::Done,
            steps: porter_core::Count(1),
            cost: porter_core::MicroUsd(0),
            report: None,
        },
    ]
}

#[tokio::test]
async fn records_reach_memory_as_cua_events_of_the_runs_session_and_space() {
    let desk = desk(vec![]).await;
    let service = Arc::new(almanac_fake::fake_service(
        almanac_fake::ScriptedConsolidator::default(),
    ));
    let cuad = links(desk.cuad_connection().await, service.clone());
    let task = cua_fake::scene::task();
    let mut task = task;
    task.app = mail();
    task.space = work();
    task.session = SessionId::parse("s-2").unwrap();
    cuad.open(&run(), &task).await;
    for record in record_of_run(&run()) {
        cuad.record(record).await.expect("recorded");
    }
    let shell = Memory::over(InProcess::new(service, Caller::ShellUi));
    let page = shell
        .timeline(
            MemSpace::parse("work").unwrap(),
            TimelineQuery {
                before: None,
                limit: Count(10),
                filter: TimelineFilter {
                    actors: ActorFilter::Everyone,
                    apps: vec![],
                    kinds: vec![],
                    trust: TrustFilter::Any,
                    range: None,
                },
            },
        )
        .await
        .expect("a timeline");
    let kinds: Vec<String> = page
        .entries
        .iter()
        .filter_map(|entry| format!("{entry:?}").split("cua.").nth(1).map(str::to_owned))
        .map(|rest| rest.split('"').next().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(page.entries.len(), 3, "{kinds:?}");

    // A record of a run nobody opened has no session to belong to.
    let stray = RunId::parse("r-9").unwrap();
    let refused = cuad.record(record_of_run(&stray).remove(2)).await;
    assert!(
        matches!(refused, Err(cua_run::LinkError::Protocol(_))),
        "{refused:?}"
    );
}

#[tokio::test]
async fn the_daemon_links_are_built_over_one_connection_and_each_reaches_its_neighbour() {
    let desk = desk(vec![yes()]).await;
    let planner = desk.as_companion().await;
    desk.open_sessions(&planner).await;
    let links = cuad::Links::over(&desk.cuad_connection().await, porter_core::Tier::Balanced);
    assert_eq!(
        links
            .gate
            .grant(GrantAsk {
                app: mail(),
                space: work()
            })
            .await,
        Ok(GrantAnswer::Granted),
        "the gate reaches intentd"
    );
    // The inbox is intentd's too, and nothing waits for the run yet.
    let waiting = links
        .events
        .inbox(InboxAsk {
            agent: AgentRef::Cua { run: run() },
            after: None,
        })
        .await;
    assert_eq!(waiting, Ok(vec![]));
    // memoryd is not on this bus: a writer carries on (the client treats a memory that is not
    // there as a no-op), so a run does not stop for it.
    let mut task = cua_fake::scene::task();
    task.app = mail();
    task.space = work();
    links.events.open(&run(), &task).await;
    let kept = links.events.record(record_of_run(&run()).remove(0)).await;
    assert_eq!(kept, Ok(()));
}

#[tokio::test]
async fn the_run_opens_its_own_session_once_and_a_withdrawn_check_leaves_the_gate_usable() {
    let desk = desk(vec![yes()]).await;
    let gate = IntentsGate::over(DbusTransport::new(desk.cuad_connection().await));
    let session = gate
        .open_session(&run(), &work())
        .await
        .expect("the cua role opens the session of its own run");
    assert_eq!(
        gate.open_session(&run(), &work()).await,
        Err(cua_run::LinkError::Refused(
            docket_core::WireRefusal::Malformed
        )),
        "a run has one session"
    );
    let _ = session;
    gate.grant(GrantAsk {
        app: mail(),
        space: work(),
    })
    .await
    .expect("granted");
    // A step that sends: the router says it will ask; the run withdraws the check instead
    // (a pause), and the sheet comes down.
    let mut asked = gate.check(step(Effect::Outbound)).await.expect("a check");
    assert!(matches!(
        asked.next().await,
        Ok(GateEvent::Confirming(_)) | Ok(GateEvent::Answered(_))
    ));
    assert_eq!(asked.close().await, Ok(()));
    drop(asked);
    assert_eq!(answered(&gate, step(Effect::Read)).await, GateAnswer::Run);
}
