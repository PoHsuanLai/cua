//! cuad on a private bus: `org.quire.Cua1` for the shell, the planner and strangers, and the
//! `org.quire.Cua` provider for the router. Each caller is a connection that owns the well-known
//! name its role is configured to have, so the identity cuad derives is the real one.

mod support;

use cua_action::{CuaAction, Summary};
use cua_bus::{BudgetBody, CuaEnvelope, CuaOutcome, CuaTask, RunEnd, WindowBody};
use cua_dbus::{CUA_BUS, ManagerProxy, RunProxy};
use cua_desktop::BackendKind;
use cua_fake::FakePdp;
use cua_fake::scene::{click, finish, reply, task, world};
use cuad::{Supervisor, serve_on};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use support::bus::PrivateBus;
use support::enabled;
use zbus::Connection;

struct Fixture {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    _serving: cuad::Serving,
    supervisor: Arc<Supervisor<cua_fake::FakeSeams>>,
    _daemon: Connection,
}

async fn named(bus: &PrivateBus, name: &str) -> Connection {
    let connection = bus.connect().await;
    connection.request_name(name).await.expect("a name");
    connection
}

async fn fixture(
    script: Vec<Result<porter_infer::CuaStepReply, cua_run::CuaModelError>>,
) -> Fixture {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    let supervisor = Arc::new(Supervisor::new(
        world(BackendKind::CosmicStock, script, FakePdp::allow_all()),
        enabled(),
    ));
    let serving = serve_on(&daemon, supervisor.clone())
        .await
        .expect("cuad serves");
    Fixture {
        _dir: dir,
        bus,
        _serving: serving,
        supervisor,
        _daemon: daemon,
    }
}

fn start_body(task: CuaTask) -> String {
    serde_json::to_string(&CuaEnvelope::current(task)).unwrap()
}

fn ask() -> CuaAction<cua_action::WindowSpace> {
    CuaAction::Ask {
        question: Summary::new("Which account?").unwrap(),
        choices: vec![],
    }
}

async fn manager(connection: &Connection) -> ManagerProxy<'_> {
    ManagerProxy::builder(connection)
        .destination(CUA_BUS)
        .unwrap()
        .build()
        .await
        .expect("a manager proxy")
}

async fn run_proxy<'a>(
    connection: &'a Connection,
    path: &zbus::zvariant::OwnedObjectPath,
) -> RunProxy<'a> {
    RunProxy::builder(connection)
        .path(path.clone())
        .unwrap()
        .build()
        .await
        .expect("a run proxy")
}

fn refusal_name(error: &zbus::Error) -> String {
    match error {
        zbus::Error::MethodError(name, _, _) => name.to_string(),
        other => panic!("not a method error: {other}"),
    }
}

#[tokio::test]
async fn the_planner_starts_a_run_the_shell_answers_it_and_the_end_is_told_on_the_bus() {
    let fx = fixture(vec![reply(ask()), reply(click()), reply(finish())]).await;
    let planner = named(&fx.bus, "org.quire.Companion1").await;
    let shell = named(&fx.bus, "org.quire.Shell").await;

    let manager_p = manager(&planner).await;
    assert_eq!(manager_p.backend().await.unwrap(), "cosmic_stock");
    let modes = manager_p.modes().await.unwrap();
    assert_eq!(modes.get("in_place").map(String::as_str), Some("hands_off"));
    assert_eq!(
        modes.get("agent_workspace").map(String::as_str),
        Some("unsupported:no_hidden_input")
    );

    // The shell listens before the run exists: the first id is known, and a signal sent before
    // anyone listens is gone.
    let path = zbus::zvariant::OwnedObjectPath::try_from("/org/quire/Cua1/run/r_1").unwrap();
    let run_s = run_proxy(&shell, &path).await;
    let mut asked = run_s.receive_ask_user().await.unwrap();
    let mut finished = run_s.receive_finished().await.unwrap();
    let mut states = run_s.receive_phase_changed().await.unwrap();

    let started = manager_p.start(&start_body(task())).await.expect("a run");
    assert_eq!(started, path);
    assert_eq!(manager_p.runs().await.unwrap(), vec![path.clone()]);

    // The question reaches the shell as a signal, labelled screen text.
    let signal = tokio::time::timeout(Duration::from_secs(10), asked.next())
        .await
        .expect("the run asks")
        .expect("a signal");
    let args = signal.args().unwrap();
    assert_eq!(args.question, 1);
    let body: CuaEnvelope<cua_bus::AskBody> = serde_json::from_str(args.ask).unwrap();
    assert_eq!(body.body.question.value, "Which account?");
    assert_eq!(
        body.body.question.label.integrity,
        prov::Integrity::Untrusted
    );

    // The properties say where the run is.
    assert_eq!(run_s.state().await.unwrap(), "waiting_for_user");
    assert_eq!(run_s.step().await.unwrap(), 1);
    assert_eq!(run_s.goal().await.unwrap(), "sign in");
    assert_eq!(run_s.mode().await.unwrap(), "in_place");
    let window: CuaEnvelope<WindowBody> =
        serde_json::from_str(&run_s.window().await.unwrap()).unwrap();
    assert_eq!(window.body.app.as_str(), "org.mozilla.firefox");
    assert_eq!(window.body.title.0, "Sign in");
    let budget: CuaEnvelope<BudgetBody> =
        serde_json::from_str(&run_s.budget().await.unwrap()).unwrap();
    assert_eq!(budget.body.limits, cua_bus::RunBudget::PROPOSED);

    // Who may answer: the shell alone.
    let stranger = bus_stranger(&fx.bus).await;
    let run_x = run_proxy(&stranger, &path).await;
    let refused = run_x.answer(1, "x").await.unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.NotPermitted");
    let run_p = run_proxy(&planner, &path).await;
    let refused = run_p.answer(1, "x").await.unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.NotPermitted");
    let refused = run_s.answer(9, "x").await.unwrap_err();
    assert_eq!(
        refusal_name(&refused),
        "org.quire.Cua1.Error.UnknownQuestion"
    );
    run_s.answer(1, "the work one").await.unwrap();

    let signal = tokio::time::timeout(Duration::from_secs(20), finished.next())
        .await
        .expect("the run ends")
        .expect("a signal");
    let end: CuaEnvelope<RunEnd> = serde_json::from_str(signal.args().unwrap().end).unwrap();
    assert_eq!(end.body.outcome, CuaOutcome::Done);
    assert!(matches!(end.body.report, cua_bus::ReportDelivery::Sent(_)));

    // The states came in order, from the lease to the end.
    let mut seen = Vec::new();
    while let Ok(Some(signal)) =
        tokio::time::timeout(Duration::from_millis(50), states.next()).await
    {
        seen.push(signal.args().unwrap().state.to_owned());
    }
    assert_eq!(
        seen,
        [
            "leasing",
            "observing",
            "thinking",
            "waiting_for_user",
            "observing",
            "thinking",
            "authorising",
            "acting",
            "settling",
            "observing",
            "thinking",
            "finished"
        ]
    );
    // And the run is no longer listed once it is over.
    let mut listed = manager_p.runs().await.unwrap();
    for _ in 0..50 {
        if listed.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
        listed = manager_p.runs().await.unwrap();
    }
    assert!(listed.is_empty());
    assert!(fx.supervisor.runs().is_empty());
}

async fn bus_stranger(bus: &PrivateBus) -> Connection {
    named(bus, "org.example.Stranger").await
}

#[tokio::test]
async fn start_and_stop_are_refused_to_the_wrong_callers_with_their_error_names() {
    let fx = fixture(vec![reply(ask())]).await;
    let shell = named(&fx.bus, "org.quire.Shell").await;
    let stranger = bus_stranger(&fx.bus).await;
    let planner = named(&fx.bus, "org.quire.Companion1").await;
    for (who, name) in [(&shell, "the shell"), (&stranger, "a stranger")] {
        let refused = manager(who)
            .await
            .start(&start_body(task()))
            .await
            .unwrap_err();
        assert_eq!(
            refusal_name(&refused),
            "org.quire.Cua1.Error.NotPermitted",
            "{name} may not start"
        );
    }
    let refused = manager(&stranger).await.stop_all().await.unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.NotPermitted");

    // A body that is not this vocabulary's is a bad task.
    let refused = manager(&planner)
        .await
        .start("{\"vocab\":7,\"body\":{}}")
        .await
        .unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.BadTask");
    let refused = manager(&planner).await.start("not json").await.unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.BadTask");
    assert_eq!(fx.supervisor.started(), 0);

    // The shell's kill switch ends what runs.
    let path = manager(&planner)
        .await
        .start(&start_body(task()))
        .await
        .unwrap();
    let run = run_proxy(&shell, &path).await;
    let mut finished = run.receive_finished().await.unwrap();
    manager(&shell).await.stop_all().await.unwrap();
    let signal = tokio::time::timeout(Duration::from_secs(20), finished.next())
        .await
        .expect("it ends")
        .expect("a signal");
    let end: CuaEnvelope<RunEnd> = serde_json::from_str(signal.args().unwrap().end).unwrap();
    assert_eq!(
        end.body.outcome,
        CuaOutcome::Cancelled(cua_bus::CancelledBy::KillSwitch)
    );
}

fn invocation(args: &[(&str, docket_core::Value)], actor: prov::Actor) -> docket_core::Invocation {
    use docket_core::{Args, CallId, Origin, ParamName, TargetValue};
    let words = |value| prov::Labelled::new(value, prov::Label::trusted_user());
    docket_core::Invocation {
        call: CallId(1),
        action: prov::ActionName::parse("cua.run.start").unwrap(),
        target: TargetValue::Nothing,
        args: args
            .iter()
            .map(|(name, value)| (ParamName::parse(name).unwrap(), words(value.clone())))
            .collect::<Args>(),
        actor,
        origin: Origin::Companion,
        space: cua_fake::scene::space(),
    }
}

fn planner_actor() -> prov::Actor {
    prov::Actor::Companion {
        session: prov::SessionId::parse("s-1").unwrap(),
        role: prov::AgentRole::Planner,
    }
}

async fn perform(
    caller: &Connection,
    invocation: &docket_core::Invocation,
) -> Result<Result<docket_core::Outcome, docket_core::AppRefusal>, zbus::Error> {
    let options: std::collections::HashMap<String, zbus::zvariant::Value<'_>> =
        std::collections::HashMap::new();
    let reply = caller
        .call_method(
            Some("org.quire.Cua"),
            "/org/quire/IntentProvider1",
            Some("org.quire.IntentProvider1"),
            "Perform",
            &(serde_json::to_string(invocation).unwrap(), options),
        )
        .await?;
    let json: String = reply.body().deserialize().unwrap();
    Ok(serde_json::from_str(&json).unwrap())
}

#[tokio::test]
async fn the_router_starts_a_run_through_the_provider_and_gets_its_id() {
    let fx = fixture(vec![reply(ask())]).await;
    let router = named(&fx.bus, "org.quire.Intents1").await;
    let window = docket_core::Value::Entity(prov::EntityId {
        app: porter_core::AppName::parse("org.quire.Cua").unwrap(),
        kind: prov::EntityKind::parse("desktop.window").unwrap(),
        key: prov::EntityKey::parse("w1").unwrap(),
    });
    let goal = docket_core::Value::Text("sign in".to_owned());
    let mode = docket_core::Value::Choice(docket_core::ChoiceId::parse("auto").unwrap());

    let outcome = perform(
        &router,
        &invocation(
            &[
                ("goal", goal.clone()),
                ("window", window.clone()),
                ("mode", mode),
            ],
            planner_actor(),
        ),
    )
    .await
    .expect("a reply")
    .expect("a run started");
    let value = outcome.value.expect("a value").value;
    assert_eq!(value, docket_core::Value::Text("r-1".to_owned()));
    // `auto` on stock is the person's own seat.
    let view = fx.supervisor.view(&"r-1".parse_run()).expect("a live run");
    assert_eq!(view.task.mode, docket_core::RunMode::InPlace);
    assert_eq!(view.task.goal.value, "sign in");
    assert_eq!(
        view.task.report.to,
        prov::Address::new(prov::AgentRef::Companion, cua_fake::scene::space()),
        "the report goes back to the planner"
    );

    // Refusals in the app's own words.
    let missing = perform(
        &router,
        &invocation(&[("window", window.clone())], planner_actor()),
    )
    .await
    .unwrap();
    assert!(
        matches!(missing, Err(docket_core::AppRefusal::NeedsParam { .. })),
        "{missing:?}"
    );
    let held = perform(
        &router,
        &invocation(
            &[("goal", goal.clone()), ("window", window)],
            planner_actor(),
        ),
    )
    .await
    .unwrap();
    assert_eq!(
        held,
        Err(docket_core::AppRefusal::Busy),
        "the window is held"
    );
    let by_hand = perform(
        &router,
        &invocation(
            &[("goal", goal)],
            prov::Actor::User {
                via: porter_core::AppName::parse("org.quire.Shell").unwrap(),
            },
        ),
    )
    .await
    .unwrap();
    assert!(
        by_hand.is_err(),
        "a run needs a window or an app, and a session to report to"
    );

    // Only the router calls a provider.
    let stranger = bus_stranger(&fx.bus).await;
    let refused = perform(&stranger, &invocation(&[], planner_actor())).await;
    assert!(refused.is_err(), "{refused:?}");
}

trait ParseRun {
    fn parse_run(&self) -> prov::RunId;
}

impl ParseRun for str {
    fn parse_run(&self) -> prov::RunId {
        prov::RunId::parse(self).unwrap()
    }
}
