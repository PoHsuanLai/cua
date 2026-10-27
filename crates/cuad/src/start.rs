//! Turning a `cua.run.start` into a task: which window or app the run works in, and the apps to
//! choose from when none was named that is open.

use crate::provider::{ModeChoice, OpenAppsAsk, StartRefusal, StartRequest};
use crate::reasons::words_of;
use cua_bus::{
    ControlRefusal, CuaCaller, CuaTask, ProcedureUse, ReportRoute, RunOrigin, SuccessCheck,
    TaskTarget,
};
use cua_desktop::WindowTrust;
use cua_desktop::{Desktop, RunMode, ToplevelInfo, ToplevelState};
use cua_host::Supervisor;
use cua_host::{Apps, Drivable};
use cua_run::choose_mode;
use porter_core::AppName;
use prov::Address;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::mpsc;

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

/// The window of `app` a run works in: the one the person is in, else the first usable one.
fn window_of_app<'a>(windows: &'a [ToplevelInfo], app: &AppName) -> Option<&'a ToplevelInfo> {
    let mut ours = windows.iter().filter(|w| usable(w) && &w.app == app);
    let first = ours.next()?;
    let focused = std::iter::once(first)
        .chain(ours)
        .find(|w| w.state.contains(&ToplevelState::Activated));
    Some(focused.unwrap_or(first))
}

/// The open apps the person allows, by id, in a stable order.
fn allowed_open_apps(windows: &[ToplevelInfo], allows: &Apps) -> Vec<AppName> {
    open_apps(windows)
        .into_iter()
        .filter(|app| allows.rule_for(app).is_some())
        .collect()
}

/// The refusal that lists the open apps the person allows (an app not on their list would only be
/// refused next); with none there is nothing to choose from.
fn choose_app(windows: &[ToplevelInfo], allows: &Apps) -> TaskRefusal {
    let apps = allowed_open_apps(windows, allows);
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
            let open = windows.as_deref().ok();
            match open.and_then(|windows| window_of_app(windows, app)) {
                Some(window) => (TaskTarget::Window(window.ident.clone()), app.clone()),
                None => {
                    if let Some(windows) = open
                        && !open_apps(windows).contains(app)
                    {
                        return Err(choose_app(windows, &allows));
                    }
                    (TaskTarget::App(app.clone()), app.clone())
                }
            }
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
        asked: request.asked.clone(),
        earlier: request.earlier.clone(),
        target,
        mode,
        space: request.space.clone(),
        session: request.session.clone(),
        conversation: request.session.clone(),
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
pub(crate) async fn starts<S: Drivable>(
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

/// Answers the provider's questions about which apps are open; with no window list, none are.
pub(crate) async fn answer_open_apps<S: Drivable>(
    supervisor: Arc<Supervisor<S>>,
    mut queue: mpsc::UnboundedReceiver<OpenAppsAsk>,
) {
    while let Some(ask) = queue.recv().await {
        let apps = match supervisor.desktop().windows().await {
            Ok(windows) => allowed_open_apps(&windows, &supervisor.apps()),
            Err(_) => Vec::new(),
        };
        let _ = ask.reply.send(apps);
    }
}
