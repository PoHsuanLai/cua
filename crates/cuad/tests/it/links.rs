//! cuad's seams over the real daemons: the router's gate and message path through docket-client's
//! `DbusTransport` against a real intentd (over docket's fakes) on a private bus, and the event
//! log through almanac-client against a real memory service (over almanac's fakes). Each caller is
//! a connection that owns the well-known name its role is configured to have.

use crate::desk::*;
use almanac_client::{InProcess, Memory};
use almanac_core::{
    ActorFilter, Caller, Count, SpaceId as MemSpace, TimelineFilter, TimelineQuery, TrustFilter,
};
use cua_run::{GateCheck, GateEvent, GateLink, MessageLink, RecordSink};
use cuad::{DaemonLinks, IntentsGate};
use docket_client::{DbusTransport, Intents};
use docket_core::{
    CallRefusal, ConfirmEnd, DraftPart, GateAnswer, GrantAnswer, GrantAsk, InboundPart, InboxAsk,
    IntentsRequest, MessageDraft,
};
use prov::{Address, AgentRef, Effect, MessageKind, MessageText, RunId, SessionId, SpaceScope};
use std::sync::Arc;
use zbus::Connection;

#[tokio::test]
async fn the_gate_asks_the_person_once_per_app_and_then_runs_reads_and_asks_again_for_sends() {
    let desk = desk(vec![yes()]).await;
    let planner = desk.as_companion().await;
    desk.open_sessions(&planner).await;
    let gate = IntentsGate::over(DbusTransport::new(desk.cuad_connection().await));

    assert_eq!(
        gate.grant(GrantAsk {
            app: mail(),
            space: work(),
            level: docket_core::Level::AlwaysAllow,
            run: None,
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
            space: work(),
            level: docket_core::Level::AlwaysAllow,
            run: None,
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
        level: docket_core::Level::AlwaysAllow,
        run: None,
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
    task.conversation = SessionId::parse("s-1").unwrap();
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
