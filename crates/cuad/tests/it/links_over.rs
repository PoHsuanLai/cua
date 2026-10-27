//! The daemon links over one connection, and the run opening its own session.

use crate::desk::*;
use cua_run::GateEvent;
use cuad::IntentsGate;
use docket_client::DbusTransport;
use docket_core::{GateAnswer, GrantAnswer, GrantAsk, InboxAsk};
use prov::{AgentRef, Effect, SessionId};

#[tokio::test]
async fn the_daemon_links_are_built_over_one_connection_and_each_reaches_its_neighbour() {
    let desk = desk(vec![yes()]).await;
    let planner = desk.as_companion().await;
    desk.open_sessions(&planner).await;
    let links = cua_system::Links::over(&desk.cuad_connection().await, porter_core::Tier::Balanced);
    assert_eq!(
        links
            .gate
            .grant(GrantAsk {
                app: mail(),
                space: work(),
                level: docket_core::Level::AlwaysAllow,
                run: None,
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
    // A conversation the router does not hold is ignored: the run opens in the start mode.
    let conversation = SessionId::parse("s-9").unwrap();
    let session = gate
        .open_session(&run(), &work(), &conversation)
        .await
        .expect("the cua role opens the session of its own run");
    assert_eq!(
        gate.open_session(&run(), &work(), &conversation).await,
        Err(cua_run::LinkError::Refused(
            docket_core::WireRefusal::Malformed
        )),
        "a run has one session"
    );
    let _ = session;
    gate.grant(GrantAsk {
        app: mail(),
        space: work(),
        level: docket_core::Level::AlwaysAllow,
        run: None,
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
