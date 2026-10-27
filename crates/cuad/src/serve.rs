//! cuad on the bus: `org.quire.Cua1` for the shell, the planner and the router, and
//! `org.quire.IntentProvider1` as `org.quire.Cua` with the one action `cua.run.start`.

use crate::bus::ManagerObject;
use crate::provider::CuaProvider;
use crate::reach::ReachObject;
use crate::relay::relay;
use crate::start::{answer_open_apps, starts};
use cua_dbus::{CUA_BUS, CUA_PATH};
use cua_host::Drivable;
use cua_host::Supervisor;
use docket_client::{ContextSource, SummonTarget};
use docket_core::{
    ContextScope, ContextSnapshot, Here, Selection, SummonAnswer, SummonOrigin, SummonSerial,
    TextTarget, Visible, WindowPrivacy,
};
use docket_dbus::PROVIDER_PATH;
use docket_dbus::callers::{CallerTable, Identity};
use futures_util::StreamExt;
use porter_core::{AppName, Count};
use prov::{Label, Labelled};
use std::sync::Arc;
use tokio::task::JoinHandle;
use zbus::fdo;

/// Why the daemon stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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
    let (provider, queues) = CuaProvider::starting(supervisor.settings().enabled).map_err(bus)?;
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
            tokio::spawn(answer_open_apps(supervisor.clone(), queues.open_apps)),
            tokio::spawn(starts(supervisor, queues.starts)),
        ],
    })
}

/// Serves cuad on the session bus until the bus goes away, naming callers by `table` (the
/// caller reads `cuad-callers.toml`, with [`crate::read_callers`], from its own environment).
pub async fn serve<S: Drivable>(
    supervisor: Supervisor<S>,
    table: CallerTable,
) -> Result<(), ServeError> {
    let connection = zbus::Connection::session().await.map_err(bus)?;
    let _serving = serve_on(&connection, Arc::new(supervisor), table).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
