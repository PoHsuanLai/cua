//! cuad on the bus: `org.quire.Cua1` for the shell, the planner and the router, and
//! `org.quire.IntentProvider1` as `org.quire.Cua` with the one action `cua.run.start`.

use crate::bus::{MANAGER_PATH, ManagerObject, RunObject, enveloped};
use crate::callers::shell_owner;
use crate::provider::{CuaProvider, ModeChoice, StartRefusal, StartRequest};
use crate::reach::ReachObject;
use crate::reasons::words_of;
use cua_bus::{
    AskBody, ControlRefusal, CuaCaller, CuaTask, ProcedureUse, ReportRoute, RunOrigin,
    SuccessCheck, TaskTarget,
};
use cua_dbus::{CUA_BUS, CUA_PATH, run_path};
use cua_desktop::WindowTrust;
use cua_desktop::{Desktop, RunMode, ToplevelInfo, ToplevelState};
use cua_host::Supervisor;
use cua_host::SupervisorEvent;
use cua_host::{Apps, Drivable};
use cua_run::{CuaSignal, choose_mode};
use docket_client::{ContextSource, SummonTarget};
use docket_core::{
    ContextScope, ContextSnapshot, Here, Selection, SummonAnswer, SummonOrigin, SummonSerial,
    TextTarget, Visible, WindowPrivacy,
};
use docket_dbus::PROVIDER_PATH;
use docket_dbus::callers::{CallerTable, Identity};
use futures_util::StreamExt;
use porter_core::{AppName, Count};
use prov::{Address, Label, Labelled, RunId};
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// Why the daemon stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServeError {
    /// The session bus could not be reached or the name could not be taken.
    #[error("bus: {0}")]
    Bus(String),
    /// `cuad-callers.toml` exists and cannot be used. A daemon that dropped it would identify
    /// fewer or more callers than it was told to.
    #[error("cuad-callers.toml: {0}")]
    Callers(String),
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

/// Why a start has no task: a refusal of the control surface, or the open apps to choose from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TaskRefusal {
    /// A refusal the control surface already has a word for.
    Control(ControlRefusal),
    /// No app was named that is open, and none could be chosen for the person: these are open
    /// (ids only, never titles), in a stable order.
    NeedsApp(Vec<AppName>),
}

impl From<ControlRefusal> for TaskRefusal {
    fn from(why: ControlRefusal) -> Self {
        TaskRefusal::Control(why)
    }
}

/// A window a run may work in: the desktop's own surfaces (the launcher, bar and dock) are not.
fn usable(window: &ToplevelInfo) -> bool {
    window.trust != WindowTrust::Shell
}

/// The apps with a usable window open, once each, in a stable order.
fn open_apps(windows: &[ToplevelInfo]) -> BTreeSet<AppName> {
    windows
        .iter()
        .filter(|w| usable(w))
        .map(|w| w.app.clone())
        .collect()
}

/// The refusal that lists the open apps the person allows (an app not on their list would only be
/// refused next); with none there is nothing to choose from.
fn choose_app(windows: &[ToplevelInfo], allows: &Apps) -> TaskRefusal {
    let apps: Vec<AppName> = open_apps(windows)
        .into_iter()
        .filter(|app| allows.rule_for(app).is_some())
        .collect();
    if apps.is_empty() {
        TaskRefusal::Control(ControlRefusal::BadTask)
    } else {
        TaskRefusal::NeedsApp(apps)
    }
}

/// The task a `cua.run.start` asks for: the window it names (its app is the window's), the app
/// alone when one of its windows is open, or the window the person is in when it names neither;
/// the mode with `auto` resolved by the setting and the backend, the settings' budget, and a
/// report back to the requester.
pub(crate) async fn task_for<S: Drivable>(
    supervisor: &Supervisor<S>,
    request: &StartRequest,
) -> Result<CuaTask, TaskRefusal> {
    let caps = supervisor.desktop().caps();
    let mode = match request.mode {
        ModeChoice::InPlace => RunMode::InPlace,
        ModeChoice::AgentWorkspace => RunMode::AgentWorkspace,
        ModeChoice::Auto => choose_mode(supervisor.settings().default_mode, &caps)
            .ok_or(ControlRefusal::ModeUnsupported)?,
    };
    let windows = supervisor.desktop().windows().await;
    if let Err(error) = &windows {
        eprintln!("cuad: the desktop did not list its windows: {error}");
    }
    let allows = supervisor.apps();
    let (target, app) = match (&request.window, &request.app) {
        (Some(ident), _) => {
            let window = windows
                .map_err(|_| ControlRefusal::WindowUnknown)?
                .into_iter()
                .find(|w| &w.ident == ident)
                .ok_or(ControlRefusal::WindowUnknown)?;
            (TaskTarget::Window(ident.clone()), window.app)
        }
        (None, Some(app)) => {
            // A desktop that cannot list its windows cannot say the app is not open.
            if let Ok(windows) = &windows
                && !open_apps(windows).contains(app)
            {
                return Err(choose_app(windows, &allows));
            }
            (TaskTarget::App(app.clone()), app.clone())
        }
        (None, None) => {
            let windows = windows.map_err(|_| ControlRefusal::BadTask)?;
            let focused = windows
                .iter()
                .find(|w| usable(w) && w.state.contains(&ToplevelState::Activated));
            match focused {
                Some(window) => (TaskTarget::Window(window.ident.clone()), window.app.clone()),
                None => return Err(choose_app(&windows, &allows)),
            }
        }
    };
    Ok(CuaTask {
        app,
        goal: request.goal.clone(),
        target,
        mode,
        space: request.space.clone(),
        session: request.session.clone(),
        budget: supervisor.settings().budget,
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
            Ok(task) => supervisor
                .start(CuaCaller::Router, task)
                .await
                .map_err(TaskRefusal::from),
            Err(refusal) => Err(refusal),
        };
        // The person's setting now says what a refusal because of it means.
        let enabled = supervisor.settings().enabled;
        let started = started.map_err(|refusal| match refusal {
            TaskRefusal::Control(why) => StartRefusal::Control {
                why,
                words: words_of(why, enabled),
            },
            TaskRefusal::NeedsApp(apps) => StartRefusal::NeedsApp(apps),
        });
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

/// Says a run's private signal to the shell and to nobody else: a destination on the message
/// keeps it from every other connection's match rules. What the model thought, what it is about
/// to type and what the screen asked are the person's to read, not the session bus's. The
/// destination is the owner of the shell's name only while its process is the shell
/// ([`shell_owner`]); a program that took the name says nothing to anyone, and hears nothing.
async fn emit_to_shell<B>(
    connection: &zbus::Connection,
    who: &Identity,
    path: &str,
    member: &str,
    body: &B,
) where
    B: serde::Serialize + zbus::zvariant::DynamicType,
{
    let Some(shell) = shell_owner(connection, who).await else {
        return;
    };
    let _ = connection
        .emit_signal(
            Some(shell.as_str()),
            path,
            "org.quire.Cua1.Run",
            member,
            body,
        )
        .await;
}

/// Says one supervisor event on the bus.
async fn say<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: &Arc<Supervisor<S>>,
    who: &Arc<Identity>,
    event: SupervisorEvent,
) {
    match event {
        SupervisorEvent::Added(run) => {
            let object = RunObject {
                supervisor: supervisor.clone(),
                id: run.clone(),
                who: who.clone(),
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
                    emit_to_shell(connection, who, &path, "ThoughtDelta", &(step.0, text.0)).await;
                }
                CuaSignal::ActionProposed(step, action) => {
                    if let Ok(body) = enveloped(action) {
                        emit_to_shell(connection, who, &path, "ActionProposed", &(step.0, body))
                            .await;
                    }
                }
                CuaSignal::ActionDone(step, outcome) => {
                    if let Ok(body) = enveloped(outcome) {
                        emit_to_shell(connection, who, &path, "ActionDone", &(step.0, body)).await;
                    }
                }
                CuaSignal::AskUser(question, ask) => {
                    let ask: AskBody = ask;
                    if let Ok(body) = enveloped(ask) {
                        emit_to_shell(connection, who, &path, "AskUser", &(question.0, body)).await;
                    }
                }
                // The run's end is told with its `RunEnd`, once the report is out. A signal
                // this build does not know has no bus member, so it is not relayed.
                _ => {}
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
                .remove::<RunObject<S, Identity>, _>(run_path(&run))
                .await;
        }
        // An event this build does not know has nothing to say on the bus.
        _ => {}
    }
}

/// The events that bring the bus's run objects (`exported`) in line with the runs the supervisor
/// has (`live`): an `Added` for each run missing an object and a `Removed` for each object whose
/// run is gone. The relay needs this after it lagged and missed some of either.
fn reconcile(exported: &BTreeSet<RunId>, live: &[RunId]) -> Vec<SupervisorEvent> {
    let live: BTreeSet<&RunId> = live.iter().collect();
    let stale = exported
        .iter()
        .filter(|run| !live.contains(run))
        .cloned()
        .map(SupervisorEvent::Removed);
    let missing = live
        .iter()
        .filter(|run| !exported.contains(**run))
        .map(|run| SupervisorEvent::Added((*run).clone()));
    stale.chain(missing).collect()
}

/// Keeps `exported` as the events leave the bus: the runs that have an object.
fn note_exported(exported: &mut BTreeSet<RunId>, event: &SupervisorEvent) {
    match event {
        SupervisorEvent::Added(run) => {
            exported.insert(run.clone());
        }
        SupervisorEvent::Removed(run) => {
            exported.remove(run);
        }
        // Signals and ends leave the run objects as they are; so does an event not known yet.
        _ => {}
    }
}

/// Relays the supervisor's events to the bus until the connection or the supervisor goes. A relay
/// that fell behind and missed events reconciles the run objects against the supervisor's runs.
async fn relay<S: Drivable>(
    connection: zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
    who: Arc<Identity>,
    mut events: broadcast::Receiver<SupervisorEvent>,
) {
    let mut exported = BTreeSet::new();
    loop {
        match events.recv().await {
            Ok(event) => {
                note_exported(&mut exported, &event);
                say(&connection, &supervisor, &who, event).await;
            }
            Err(broadcast::error::RecvError::Lagged(missed)) => {
                eprintln!(
                    "cuad: the bus relay missed {missed} events; reconciling the run objects"
                );
                for event in reconcile(&exported, &supervisor.runs()) {
                    note_exported(&mut exported, &event);
                    say(&connection, &supervisor, &who, event).await;
                }
            }
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

/// Who the provider answers: intentd by the system's `/proc` in the daemon, or whom a test's
/// identity says is intentd.
enum Answering {
    IntentdOnly,
    Told(Arc<Identity>),
}

/// Serves `org.quire.Cua1` (`Manager` at `/org/quire/Cua1`, a `Run` per live run at
/// `/org/quire/Cua1/run/<id>`) and `org.quire.IntentProvider1` as `org.quire.Cua` on
/// `connection`. Each caller's role is its process's, named by `table` (`cuad-callers.toml`) over
/// the system's `/proc`, never a bus name it owns; the provider answers intentd's unit alone.
/// Returns once the names are ours; the connection keeps serving until the returned guard is
/// dropped.
pub async fn serve_on<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
    table: CallerTable,
) -> Result<Serving, ServeError> {
    let who = Arc::new(Identity::on_system(connection.clone(), table));
    serve_as(connection, supervisor, who, Answering::IntentdOnly).await
}

/// [`serve_on`] with callers named by `who`, the provider answering the process `who` names as
/// intentd: the daemon's way in, where `who` reads the system's `/proc` (or a test build's
/// fixture tree, which the provider must read too, or intentd is a stranger to it).
pub async fn serve_on_as<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
    who: Identity,
) -> Result<Serving, ServeError> {
    let who = Arc::new(who);
    serve_as(connection, supervisor, who.clone(), Answering::Told(who)).await
}

/// [`serve_on`] with callers named by `who` (a test's fixture proc root and introductions), which
/// also says who the provider takes for intentd.
pub async fn serve_on_with<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
    who: Arc<Identity>,
) -> Result<Serving, ServeError> {
    serve_as(connection, supervisor, who.clone(), Answering::Told(who)).await
}

async fn serve_as<S: Drivable>(
    connection: &zbus::Connection,
    supervisor: Arc<Supervisor<S>>,
    who: Arc<Identity>,
    answering: Answering,
) -> Result<Serving, ServeError> {
    // Subscribed before any name is taken, so no run is missed.
    let events = supervisor.subscribe();
    connection
        .object_server()
        .at(
            CUA_PATH,
            ManagerObject {
                supervisor: supervisor.clone(),
                who: who.clone(),
            },
        )
        .await
        .map_err(bus)?;
    // `org.quire.IntentReach1` beside `IntentProvider1`: the live value of the one action.
    connection
        .object_server()
        .at(
            PROVIDER_PATH,
            ReachObject {
                supervisor: supervisor.clone(),
            },
        )
        .await
        .map_err(bus)?;
    let (provider, queue) = CuaProvider::starting(supervisor.settings().enabled).map_err(bus)?;
    match answering {
        Answering::IntentdOnly => {
            docket_client::serve_on(connection, provider, NoContext, NoSummon)
                .await
                .map_err(bus)?;
        }
        Answering::Told(intentd) => {
            docket_client::serve_on_with(connection, provider, NoContext, NoSummon, intentd)
                .await
                .map_err(bus)?;
        }
    }
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
            tokio::spawn(relay(connection.clone(), supervisor.clone(), who, events)),
            tokio::spawn(starts(supervisor, queue)),
        ],
    })
}

/// Serves cuad on the session bus until the bus goes away.
pub async fn serve<S: Drivable>(supervisor: Supervisor<S>) -> Result<(), ServeError> {
    let connection = zbus::Connection::session().await.map_err(bus)?;
    let table = crate::callers::read(&|key: &str| std::env::var(key).ok())
        .map_err(|e| ServeError::Callers(e.to_string()))?;
    let _serving = serve_on(&connection, Arc::new(supervisor), table).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_host::mint_run_id;

    fn runs(ns: &[u64]) -> Vec<RunId> {
        ns.iter().copied().map(mint_run_id).collect()
    }

    #[test]
    fn a_lagged_relay_removes_stale_objects_and_adds_missed_ones() {
        let exported: BTreeSet<RunId> = runs(&[1, 2]).into_iter().collect();
        let events = reconcile(&exported, &runs(&[2, 3]));
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], SupervisorEvent::Removed(r) if *r == mint_run_id(1)));
        assert!(matches!(&events[1], SupervisorEvent::Added(r) if *r == mint_run_id(3)));
    }

    #[test]
    fn nothing_to_reconcile_when_the_objects_match_the_runs() {
        let exported: BTreeSet<RunId> = runs(&[1, 2]).into_iter().collect();
        assert!(reconcile(&exported, &runs(&[1, 2])).is_empty());
    }

    #[test]
    fn exported_follows_added_and_removed() {
        let mut exported = BTreeSet::new();
        note_exported(&mut exported, &SupervisorEvent::Added(mint_run_id(1)));
        assert!(exported.contains(&mint_run_id(1)));
        note_exported(&mut exported, &SupervisorEvent::Removed(mint_run_id(1)));
        assert!(exported.is_empty());
    }
}
