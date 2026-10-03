//! cuad on the bus: `org.quire.Cua1` for the shell, the planner and the router, and
//! `org.quire.IntentProvider1` as `org.quire.Cua` with the one action `cua.run.start`.

use crate::bus::{MANAGER_PATH, ManagerObject, RunObject, enveloped};
use crate::drive::Drivable;
use crate::events::SupervisorEvent;
use crate::provider::{CuaProvider, ModeChoice, StartRequest};
use crate::supervisor::Supervisor;
use cua_bus::{
    AskBody, ControlRefusal, CuaCaller, CuaTask, ProcedureUse, ReportRoute, RunOrigin,
    SuccessCheck, TaskTarget,
};
use cua_dbus::{CUA_BUS, CUA_PATH, run_path};
use cua_desktop::{Desktop, RunMode};
use cua_run::{CuaSignal, choose_mode};
use docket_client::{ContextSource, SummonTarget};
use docket_core::{
    ContextScope, ContextSnapshot, Here, Selection, SummonAnswer, SummonOrigin, SummonSerial,
    TextTarget, Visible, WindowPrivacy,
};
use futures_util::StreamExt;
use porter_core::{AppName, Count};
use prov::{Address, Label, Labelled};
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// Why the daemon stopped.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The session bus could not be reached or the name could not be taken.
    #[error("bus: {0}")]
    Bus(String),
}

fn bus<E: std::fmt::Display>(error: E) -> ServeError {
    ServeError::Bus(error.to_string())
}

/// cuad shows the app's context to nobody: it has no windows of its own.
struct NoContext;

impl ContextSource for NoContext {
    fn snapshot(&self, scope: ContextScope) -> ContextSnapshot {
        let ContextScope::ActiveWindow = scope;
        ContextSnapshot {
            app: AppName::parse("org.quire.Cua").expect("an app name"),
            window: Labelled::new(String::new(), Label::trusted_user()),
            here: Here::Nowhere,
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: Vec::new(),
                total: Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Private,
        }
    }
}

/// Nor is there a prompt to summon in it.
struct NoSummon;

impl SummonTarget for NoSummon {
    fn summon(&self, _serial: SummonSerial, _origin: SummonOrigin) -> SummonAnswer {
        SummonAnswer::Declined
    }
}

/// The task a `cua.run.start` asks for: the window it names (its app is the window's) or the
/// app alone, the mode with `auto` resolved by the setting and the backend, the settings'
/// budget, and a report back to the requester.
pub(crate) async fn task_for<S: Drivable>(
    supervisor: &Supervisor<S>,
    request: &StartRequest,
) -> Result<CuaTask, ControlRefusal> {
    let caps = supervisor.desktop().caps();
    let mode = match request.mode {
        ModeChoice::InPlace => RunMode::InPlace,
        ModeChoice::AgentWorkspace => RunMode::AgentWorkspace,
        ModeChoice::Auto => choose_mode(supervisor.config().settings.default_mode, &caps)
            .ok_or(ControlRefusal::ModeUnsupported)?,
    };
    let (target, app) = match (&request.window, &request.app) {
        (Some(ident), _) => {
            let windows = supervisor
                .desktop()
                .windows()
                .await
                .map_err(|_| ControlRefusal::WindowUnknown)?;
            let window = windows
                .into_iter()
                .find(|w| &w.ident == ident)
                .ok_or(ControlRefusal::WindowUnknown)?;
            (TaskTarget::Window(ident.clone()), window.app)
        }
        (None, Some(app)) => (TaskTarget::App(app.clone()), app.clone()),
        (None, None) => return Err(ControlRefusal::BadTask),
    };
    Ok(CuaTask {
        app,
        goal: request.goal.clone(),
        target,
        mode,
        space: request.space.clone(),
        session: request.session.clone(),
        budget: supervisor.config().settings.budget,
        data_in: Vec::new(),
        success: SuccessCheck::ModelSays,
        origin: RunOrigin::Planner,
        procedure: ProcedureUse::None,
        report: ReportRoute {
            to: Address::new(request.from.clone(), request.space.clone()),
            thread: None,
            in_reply_to: None,
        },
    })
}

/// Turns the starts the provider hands over into runs, answering each with its id.
async fn starts<S: Drivable>(
    supervisor: Arc<Supervisor<S>>,
    mut queue: mpsc::UnboundedReceiver<StartRequest>,
) {
    while let Some(request) = queue.recv().await {
        let started = match task_for(&supervisor, &request).await {
            Ok(task) => supervisor.start(CuaCaller::Router, task).await,
            Err(refusal) => Err(refusal),
        };
        let _ = request.reply.send(started);
    }
}

fn manager_signal_path() -> &'static str {
    MANAGER_PATH
}

async fn emit_run<B>(connection: &zbus::Connection, path: &str, member: &str, body: &B)
where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let _ = connection
        .emit_signal(None::<&str>, path, "org.quire.Cua1.Run", member, body)
        .await;
}

/// Says one supervisor event on the bus.
async fn say<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: &Arc<Supervisor<S>>,
    event: SupervisorEvent,
) {
    match event {
        SupervisorEvent::Added(run) => {
            let object = RunObject {
                supervisor: supervisor.clone(),
                id: run.clone(),
                names: supervisor.config().callers.clone(),
            };
            let _ = connection.object_server().at(run_path(&run), object).await;
            if let Ok(path) = OwnedObjectPath::try_from(run_path(&run)) {
                let _ = connection
                    .emit_signal(
                        None::<&str>,
                        manager_signal_path(),
                        "org.quire.Cua1.Manager",
                        "RunAdded",
                        &(path,),
                    )
                    .await;
            }
        }
        SupervisorEvent::Signal(run, signal) => {
            let path = run_path(&run);
            match signal {
                CuaSignal::StateChanged(slug) => {
                    let state = cua_bus::slug_of(&slug).unwrap_or_default();
                    emit_run(connection, &path, "StateChanged", &(state,)).await;
                }
                CuaSignal::StepStarted(step) => {
                    emit_run(connection, &path, "StepStarted", &(step.0,)).await;
                }
                CuaSignal::ThoughtDelta(step, text) => {
                    emit_run(connection, &path, "ThoughtDelta", &(step.0, text.0)).await;
                }
                CuaSignal::ActionProposed(step, action) => {
                    if let Ok(body) = enveloped(action) {
                        emit_run(connection, &path, "ActionProposed", &(step.0, body)).await;
                    }
                }
                CuaSignal::ActionDone(step, outcome) => {
                    if let Ok(body) = enveloped(outcome) {
                        emit_run(connection, &path, "ActionDone", &(step.0, body)).await;
                    }
                }
                CuaSignal::AskUser(question, ask) => {
                    let ask: AskBody = ask;
                    if let Ok(body) = enveloped(ask) {
                        emit_run(connection, &path, "AskUser", &(question.0, body)).await;
                    }
                }
                // The run's end is told with its `RunEnd`, once the report is out.
                CuaSignal::Finished(_) => {}
            }
        }
        SupervisorEvent::Ended(run, end) => {
            if let Ok(body) = enveloped(*end) {
                emit_run(connection, &run_path(&run), "Finished", &(body,)).await;
            }
        }
        SupervisorEvent::Removed(run) => {
            if let Ok(path) = OwnedObjectPath::try_from(run_path(&run)) {
                let _ = connection
                    .emit_signal(
                        None::<&str>,
                        manager_signal_path(),
                        "org.quire.Cua1.Manager",
                        "RunRemoved",
                        &(path,),
                    )
                    .await;
            }
            let _ = connection
                .object_server()
                .remove::<RunObject<S>, _>(run_path(&run))
                .await;
        }
    }
}

/// Relays the supervisor's events to the bus until the connection or the supervisor goes.
async fn relay<S: Drivable>(
    connection: zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
    mut events: broadcast::Receiver<SupervisorEvent>,
) {
    loop {
        match events.recv().await {
            Ok(event) => say(&connection, &supervisor, event).await,
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// cuad on a connection: its objects are up and its names are owned while this lives.
#[derive(Debug)]
pub struct Serving {
    tasks: Vec<JoinHandle<()>>,
}

impl Drop for Serving {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Serves `org.quire.Cua1` (`Manager` at `/org/quire/Cua1`, a `Run` per live run at
/// `/org/quire/Cua1/run/<id>`) and `org.quire.IntentProvider1` as `org.quire.Cua` on
/// `connection`, with each caller's role derived from the connection. Returns once the names are
/// ours; the connection keeps serving until the returned guard is dropped.
pub async fn serve_on<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
) -> Result<Serving, ServeError> {
    let names = supervisor.config().callers.clone();
    // Subscribed before any name is taken, so no run is missed.
    let events = supervisor.subscribe();
    connection
        .object_server()
        .at(
            CUA_PATH,
            ManagerObject {
                supervisor: supervisor.clone(),
                names,
            },
        )
        .await
        .map_err(bus)?;
    let (provider, queue) =
        CuaProvider::starting(supervisor.config().settings.enabled).map_err(bus)?;
    docket_client::serve_on(connection, provider, NoContext, NoSummon)
        .await
        .map_err(bus)?;
    let taken = connection
        .request_name_with_flags(CUA_BUS, fdo::RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(bus)?;
    match taken {
        fdo::RequestNameReply::PrimaryOwner | fdo::RequestNameReply::AlreadyOwner => {}
        fdo::RequestNameReply::InQueue | fdo::RequestNameReply::Exists => {
            return Err(ServeError::Bus(format!("{CUA_BUS} is taken")));
        }
    }
    Ok(Serving {
        tasks: vec![
            tokio::spawn(relay(connection.clone(), supervisor.clone(), events)),
            tokio::spawn(starts(supervisor, queue)),
        ],
    })
}

/// Serves cuad on the session bus until the bus goes away.
pub async fn serve<S: Drivable>(supervisor: Supervisor<S>) -> Result<(), ServeError> {
    let connection = zbus::Connection::session().await.map_err(bus)?;
    let _serving = serve_on(&connection, Arc::new(supervisor)).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
