//! cuad on a private bus: `org.quire.Cua1` for the shell, the planner and strangers, and the
//! `org.quire.Cua` provider for the router. Every connection of a test is one process, so each
//! caller is introduced as the app and role its process would have in cuad's callers table
//! (`Who`); a bus name decides nothing, and a program that owns `org.quire.Shell` is only the
//! shell if its process is.

use crate::support::bus::PrivateBus;
use crate::support::enabled;
use cua_action::{CuaAction, Summary};
use cua_bus::{BudgetBody, CuaEnvelope, CuaOutcome, CuaTask, RunEnd, WindowBody};
use cua_dbus::{CUA_BUS, ManagerProxy, RunProxy};
use cua_desktop::BackendKind;
use cua_fake::FakePdp;
use cua_fake::scene::{click, finish, reply, task, world};
use cuad::{SHELL_BUS, Supervisor, serve_on_with, shipped_callers};
use docket_dbus::callers::{Identity, Role};
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;
use zbus::Connection;

struct Fixture {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    _serving: cuad::Serving,
    who: Arc<Identity>,
    supervisor: Arc<Supervisor<cua_fake::FakeSeams>>,
    _daemon: Connection,
}

/// Who a caller's process is, as cuad's callers table would name it.
#[derive(Clone, Copy)]
enum Who {
    Router,
    Planner,
    Shell,
    Stranger,
    /// A program that took `org.quire.Shell` while the shell restarted: an app, not the shell.
    Squatter,
}

impl Who {
    fn standing(self) -> (&'static str, Role) {
        match self {
            Who::Router => ("org.quire.Intents", Role::Agent),
            Who::Planner => ("org.quire.Companion1", Role::Agent),
            Who::Shell => ("org.quire.Shell", Role::SheetHost),
            Who::Stranger => ("org.example.Stranger", Role::App),
            Who::Squatter => ("org.example.Squatter", Role::App),
        }
    }

    /// The well-known name it owns: the shell's, for the shell and for its squatter.
    fn name(self) -> Option<&'static str> {
        match self {
            Who::Shell | Who::Squatter => Some(SHELL_BUS),
            Who::Router | Who::Planner | Who::Stranger => None,
        }
    }
}

async fn named(fx: &Fixture, who: Who) -> Connection {
    let connection = fx.bus.connect().await;
    if let Some(name) = who.name() {
        connection.request_name(name).await.expect("a name");
    }
    let (app, role) = who.standing();
    fx.who.introduce_app(&connection, app, role);
    connection
}

async fn fixture(
    script: Vec<Result<porter_infer::CuaStepReply, cua_run::CuaModelError>>,
) -> Fixture {
    fixture_in(world(BackendKind::KwinStock, script, FakePdp::allow_all())).await
}

async fn fixture_in(world: cua_fake::World) -> Fixture {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    let supervisor = Arc::new(Supervisor::new(world, enabled()));
    // The fixture tree is empty: nobody is identified but whom a test introduces.
    let who = Arc::new(Identity::on_fixture(
        daemon.clone(),
        shipped_callers(),
        dir.path().join("proc"),
    ));
    let serving = serve_on_with(&daemon, supervisor.clone(), who.clone())
        .await
        .expect("cuad serves");
    Fixture {
        _dir: dir,
        bus,
        _serving: serving,
        who,
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
    let planner = named(&fx, Who::Planner).await;
    let shell = named(&fx, Who::Shell).await;

    let manager_p = manager(&planner).await;
    assert_eq!(manager_p.backend().await.unwrap(), "kwin_stock");
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
    // The id is a property: the path spelling (`r_1`) is lossy, the slug is `r-1`.
    assert_eq!(run_s.id().await.unwrap(), "r-1");
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
    let stranger = named(&fx, Who::Stranger).await;
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

    // The fixture window is a sign-in form with a password field, so the click that follows is
    // held for a question of the run's own, with the next id.
    let signal = tokio::time::timeout(Duration::from_secs(10), asked.next())
        .await
        .expect("the run asks again before the click")
        .expect("a signal");
    assert_eq!(signal.args().unwrap().question, 2);
    run_s.answer(2, cua_run::FORCED_YES).await.unwrap();

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
            "waiting_for_user",
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

#[tokio::test]
async fn start_and_stop_are_refused_to_the_wrong_callers_with_their_error_names() {
    let fx = fixture(vec![reply(ask())]).await;
    let shell = named(&fx, Who::Shell).await;
    let stranger = named(&fx, Who::Stranger).await;
    let planner = named(&fx, Who::Planner).await;
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
    let router = named(&fx, Who::Router).await;
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
    assert_eq!(view.task.mode, cua_action::RunMode::InPlace);
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
    let stranger = named(&fx, Who::Stranger).await;
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

#[tokio::test]
async fn what_the_model_proposes_and_the_screen_asks_reaches_the_shell_and_no_one_else() {
    let fx = fixture(vec![reply(ask()), reply(finish())]).await;
    let planner = named(&fx, Who::Planner).await;
    let shell = named(&fx, Who::Shell).await;
    let stranger = named(&fx, Who::Stranger).await;
    let path = zbus::zvariant::OwnedObjectPath::try_from("/org/quire/Cua1/run/r_1").unwrap();
    let mut shell_asked = run_proxy(&shell, &path)
        .await
        .receive_ask_user()
        .await
        .unwrap();
    let spy = run_proxy(&stranger, &path).await;
    let mut spy_asked = spy.receive_ask_user().await.unwrap();
    let mut spy_proposed = spy.receive_action_proposed().await.unwrap();
    let mut spy_states = spy.receive_phase_changed().await.unwrap();

    manager(&planner)
        .await
        .start(&start_body(task()))
        .await
        .expect("a run");
    tokio::time::timeout(Duration::from_secs(10), shell_asked.next())
        .await
        .expect("the run asks the shell")
        .expect("a signal");
    // Broadcast state changes still reach a stranger; the question and the proposals do not.
    tokio::time::timeout(Duration::from_secs(10), spy_states.next())
        .await
        .expect("a state change is public")
        .expect("a signal");
    let quiet = Duration::from_millis(500);
    assert!(
        tokio::time::timeout(quiet, spy_asked.next()).await.is_err(),
        "the question is the shell's"
    );
    assert!(
        tokio::time::timeout(quiet, spy_proposed.next())
            .await
            .is_err(),
        "the proposed action is the shell's"
    );
    fx.supervisor
        .stop_all(cua_bus::CuaCaller::Shell)
        .await
        .unwrap();
}

#[tokio::test]
async fn a_program_that_took_the_shells_name_is_not_the_shell() {
    let fx = fixture(vec![reply(ask()), reply(finish())]).await;
    let planner = named(&fx, Who::Planner).await;
    let squatter = named(&fx, Who::Squatter).await;
    let path = zbus::zvariant::OwnedObjectPath::try_from("/org/quire/Cua1/run/r_1").unwrap();
    let spy = run_proxy(&squatter, &path).await;
    let mut spy_asked = spy.receive_ask_user().await.unwrap();
    let mut spy_proposed = spy.receive_action_proposed().await.unwrap();
    let mut spy_thoughts = spy.receive_thought_delta().await.unwrap();
    let mut spy_states = spy.receive_phase_changed().await.unwrap();

    // Owning the name makes it neither the shell nor the planner.
    let refused = manager(&squatter)
        .await
        .start(&start_body(task()))
        .await
        .unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.NotPermitted");
    assert_eq!(fx.supervisor.started(), 0);

    manager(&planner)
        .await
        .start(&start_body(task()))
        .await
        .expect("a run");
    // The run reaches its question, which only the shell is told.
    loop {
        let state = tokio::time::timeout(Duration::from_secs(10), spy_states.next())
            .await
            .expect("the run asks")
            .expect("a signal");
        if state.args().unwrap().state == "waiting_for_user" {
            break;
        }
    }
    let quiet = Duration::from_millis(500);
    assert!(
        tokio::time::timeout(quiet, spy_asked.next()).await.is_err(),
        "the question is the shell's, not the name's owner's"
    );
    assert!(
        tokio::time::timeout(quiet, spy_proposed.next())
            .await
            .is_err(),
        "the proposed action is the shell's"
    );
    assert!(
        tokio::time::timeout(quiet, spy_thoughts.next())
            .await
            .is_err(),
        "the thoughts are the shell's"
    );
    let refused = spy.answer(1, "yes").await.unwrap_err();
    assert_eq!(refusal_name(&refused), "org.quire.Cua1.Error.NotPermitted");
    fx.supervisor
        .stop_all(cua_bus::CuaCaller::Shell)
        .await
        .unwrap();
}

/// A desktop with the lamp page's browser (focused, unless `focus` says otherwise) and a drawing
/// app, whose titles hold private text.
async fn two_windows(focus: Option<&str>) -> Fixture {
    use cua_desktop::{ToplevelIdent, ToplevelState, WindowTitle};
    let open = |ident: &str, app: &str, title: &str| {
        let mut window = cua_fake::scene::window();
        window.ident = ToplevelIdent::parse(ident).unwrap();
        window.app = porter_core::AppName::parse(app).unwrap();
        window.title = WindowTitle(title.to_owned());
        window.state = if focus == Some(ident) {
            std::collections::BTreeSet::from([ToplevelState::Activated])
        } else {
            std::collections::BTreeSet::new()
        };
        window
    };
    let mut shell = open("launcher", "org.quire.Shell", "Launcher");
    shell.trust = cua_action::WindowTrust::Shell;
    shell.state = std::collections::BTreeSet::from([ToplevelState::Activated]);
    let windows = vec![
        open("w-browser", "app.zen_browser.zen", "secret lamp order"),
        open("w-draw-0", "org.example.Draw", "secret notes"),
        open("w-draw", "org.example.Draw", "secret sketch"),
        shell,
    ];
    let mut world = world(
        BackendKind::KwinStock,
        vec![reply(ask())],
        FakePdp::allow_all(),
    );
    world.desktop = cua_fake::FakeDesktop::new(BackendKind::KwinStock, windows);
    fixture_in(world).await
}

async fn start_with(
    fx: &Fixture,
    app: Option<&str>,
) -> Result<docket_core::Outcome, docket_core::AppRefusal> {
    start_with_value(fx, app.map(|app| docket_core::Value::Text(app.to_owned()))).await
}

async fn start_with_value(
    fx: &Fixture,
    app: Option<docket_core::Value>,
) -> Result<docket_core::Outcome, docket_core::AppRefusal> {
    let router = named(fx, Who::Router).await;
    let mut args = vec![("goal", docket_core::Value::Text("draw the lamp".to_owned()))];
    args.extend(app.map(|app| ("app", app)));
    perform(&router, &invocation(&args, planner_actor()))
        .await
        .expect("a reply")
}

async fn suggest_apps(fx: &Fixture, typed: &str, param: &str) -> Vec<String> {
    let router = named(fx, Who::Router).await;
    let ask = docket_core::SuggestAsk {
        action: docket_core::ActionRef {
            app: porter_core::AppName::parse("org.quire.Cua").unwrap(),
            name: prov::ActionName::parse("cua.run.start").unwrap(),
        },
        param: docket_core::ParamName::parse(param).unwrap(),
        typed: typed.to_owned(),
    };
    let reply = router
        .call_method(
            Some("org.quire.Cua"),
            "/org/quire/IntentProvider1",
            Some("org.quire.IntentProvider1"),
            "Suggest",
            &(serde_json::to_string(&ask).unwrap(),),
        )
        .await
        .expect("a reply");
    let json: String = reply.body().deserialize().unwrap();
    let options: Vec<docket_core::EntityRef> = serde_json::from_str(&json).unwrap();
    options
        .iter()
        .map(|o| o.id.key.as_str().to_owned())
        .collect()
}

#[tokio::test]
async fn the_open_apps_are_offered_as_choices_for_app_and_for_nothing_else() {
    let fx = two_windows(None).await;
    assert_eq!(
        suggest_apps(&fx, "", "app").await,
        ["app.zen_browser.zen", "org.example.Draw"],
        "ids only, not the shell's own"
    );
    assert_eq!(suggest_apps(&fx, "DRAW", "app").await, ["org.example.Draw"]);
    assert!(suggest_apps(&fx, "", "goal").await.is_empty());
}

#[tokio::test]
async fn the_app_parameter_reads_as_text_or_an_entity() {
    let draw = "org.example.Draw";
    let forms = [
        docket_core::Value::Text(draw.to_owned()),
        docket_core::Value::Entity(prov::EntityId {
            app: porter_core::AppName::parse("org.quire.Cua").unwrap(),
            kind: prov::EntityKind::parse("desktop.app").unwrap(),
            key: prov::EntityKey::parse(draw).unwrap(),
        }),
    ];
    for form in forms {
        let fx = two_windows(None).await;
        start_with_value(&fx, Some(form.clone()))
            .await
            .unwrap_or_else(|e| panic!("{form:?}: {e:?}"));
        let view = fx.supervisor.view(&"r-1".parse_run()).expect("a live run");
        assert_eq!(view.task.app.as_str(), draw, "{form:?}");
    }
}

#[tokio::test]
async fn an_app_that_is_not_open_is_refused_with_the_open_apps_by_id_and_never_a_title() {
    let fx = two_windows(None).await;
    let Err(docket_core::AppRefusal::NeedsParam { param, options }) =
        start_with(&fx, Some("org.quire.Desktop")).await
    else {
        panic!("a refusal with choices");
    };
    assert_eq!(param.as_str(), "app");
    let ids: Vec<&str> = options.iter().map(|o| o.id.key.as_str()).collect();
    assert_eq!(
        ids,
        ["app.zen_browser.zen", "org.example.Draw"],
        "sorted, once each, and not the shell's own"
    );
    let said = format!("{options:?}");
    assert!(!said.contains("secret"), "{said}");
}

#[tokio::test]
async fn no_app_means_the_window_the_person_is_in_and_a_known_app_still_starts() {
    let fx = two_windows(Some("w-draw")).await;
    start_with(&fx, None).await.expect("the focused window");
    let view = fx.supervisor.view(&"r-1".parse_run()).expect("a live run");
    assert_eq!(view.task.app.as_str(), "org.example.Draw");

    // An open app named alone runs on its window: the one the person is in, else the first.
    let fx = two_windows(Some("w-draw")).await;
    start_with(&fx, Some("org.example.Draw"))
        .await
        .expect("an open app starts");
    let view = fx.supervisor.view(&"r-1".parse_run()).expect("a live run");
    assert_eq!(
        view.task.target,
        cua_bus::TaskTarget::Window(ident("w-draw"))
    );

    let fx = two_windows(None).await;
    start_with(&fx, Some("org.example.Draw"))
        .await
        .expect("an open app starts");
    let view = fx.supervisor.view(&"r-1".parse_run()).expect("a live run");
    assert_eq!(
        view.task.target,
        cua_bus::TaskTarget::Window(ident("w-draw-0"))
    );
}

fn ident(text: &str) -> cua_desktop::ToplevelIdent {
    cua_desktop::ToplevelIdent::parse(text).unwrap()
}

#[tokio::test]
async fn no_app_and_no_focus_gives_the_choices_even_when_the_shell_has_it() {
    let fx = two_windows(None).await;
    let refused = start_with(&fx, None).await;
    assert!(
        matches!(refused, Err(docket_core::AppRefusal::NeedsParam { ref options, .. }) if options.len() == 2),
        "{refused:?}"
    );
    // An open app the person has not listed is no choice: it would only be refused next.
    fx.supervisor.apply_settings(&cuad::CuaSettings {
        config: fx.supervisor.settings(),
        apps: cuad::Apps::Listed(vec![cuad::AppRule {
            app: prov::AppName::parse("app.zen_browser.zen").expect("an app"),
            allow: cuad::AppAllow::Ask,
        }]),
    });
    let Err(docket_core::AppRefusal::NeedsParam { options, .. }) = start_with(&fx, None).await
    else {
        panic!("a refusal with choices");
    };
    let ids: Vec<&str> = options.iter().map(|o| o.id.key.as_str()).collect();
    assert_eq!(ids, ["app.zen_browser.zen"]);
}
