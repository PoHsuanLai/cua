//! `org.quire.Cua1` served: the Manager at `/org/quire/Cua1` and one Run per live run. The member
//! list, its signatures and its signals are those of `cua-dbus`'s skeletons (a test holds the
//! introspection of these objects to `dbus/org.quire.Cua1.xml`).
//!
//! Bodies are JSON in an `s` argument, in the `CuaEnvelope` where a body is sent. Who is calling
//! is derived from the process behind the message's sender (`callers`), never from a bus name it
//! owns and never from anything in a body.

use crate::callers;
use cua_bus::{
    CuaCaller, CuaEnvelope, CuaMember, CuaTask, CuaVocab, QuestionId, WindowBody, backend_wire,
    modes_wire, slug_of,
};
use cua_dbus::{CUA_PATH, CuaError, run_path};
use cua_desktop::{Desktop, RunMode, WindowTitle};
use cua_host::Drivable;
use cua_host::Supervisor;
use cua_run::mode_support;
use docket_dbus::callers::Callers;
use prov::{Label, Labelled, RunId};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use zbus::fdo;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::OwnedObjectPath;

/// The JSON of `body` in the envelope of this vocabulary.
pub(crate) fn enveloped<T: Serialize>(body: T) -> fdo::Result<String> {
    serde_json::to_string(&CuaEnvelope::current(body))
        .map_err(|e| fdo::Error::Failed(e.to_string()))
}

/// Who is calling: the process behind the header's sender decides.
async fn caller_of(who: &impl Callers, header: &Header<'_>) -> CuaCaller {
    callers::caller_of(who, header.sender().map(|sender| sender.as_str())).await
}

fn object_path(run: &RunId) -> OwnedObjectPath {
    OwnedObjectPath::try_from(run_path(run)).expect("a run path is an object path")
}

/// `org.quire.Cua1.Manager`.
pub(crate) struct ManagerObject<S: Drivable, K: Callers> {
    pub supervisor: Arc<Supervisor<S>>,
    pub who: Arc<K>,
}

#[zbus::interface(name = "org.quire.Cua1.Manager")]
impl<S: Drivable, K: Callers> ManagerObject<S, K> {
    #[zbus(out_args("run"))]
    async fn start(
        &self,
        task: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<OwnedObjectPath, CuaError> {
        let caller = caller_of(&self.who, &header).await;
        let envelope: CuaEnvelope<CuaTask> =
            serde_json::from_str(&task).map_err(|e| CuaError::BadTask(e.to_string()))?;
        if envelope.vocab != CuaVocab::CURRENT {
            return Err(CuaError::BadTask(
                "the vocabulary is not one this cuad speaks".into(),
            ));
        }
        let id = self.supervisor.start(caller, envelope.body).await?;
        // Registered before the id goes back, so the caller can use it at once.
        connection
            .object_server()
            .at(
                run_path(&id),
                RunObject {
                    supervisor: self.supervisor.clone(),
                    id: id.clone(),
                    who: self.who.clone(),
                },
            )
            .await?;
        if self.supervisor.view(&id).is_none() {
            // The run was over before the object was up (a run that ends at once): the relay
            // has already said it is removed, so nothing would take the object down later.
            connection
                .object_server()
                .remove::<RunObject<S, K>, _>(run_path(&id))
                .await?;
        }
        Ok(object_path(&id))
    }

    async fn runs(&self) -> Vec<OwnedObjectPath> {
        self.supervisor.runs().iter().map(object_path).collect()
    }

    async fn stop_all(&self, #[zbus(header)] header: Header<'_>) -> Result<(), CuaError> {
        let caller = caller_of(&self.who, &header).await;
        Ok(self.supervisor.stop_all(caller).await?)
    }

    #[zbus(property)]
    async fn backend(&self) -> String {
        backend_wire(self.supervisor.desktop().backend())
    }

    #[zbus(property)]
    async fn modes(&self) -> HashMap<String, String> {
        // No backend, no mode: the property is empty rather than promising what is not there.
        if self.supervisor.desktop().backend().is_none() {
            return HashMap::new();
        }
        let caps = self.supervisor.desktop().caps();
        let support: Vec<_> = [
            RunMode::InPlace,
            RunMode::AgentWorkspace,
            RunMode::NestedSession,
        ]
        .into_iter()
        .map(|mode| (mode, mode_support(&caps, mode)))
        .collect();
        modes_wire(&support).into_iter().collect()
    }

    #[zbus(property)]
    async fn sight_note(&self) -> String {
        self.supervisor
            .sight_note()
            .await
            .map(str::to_owned)
            .unwrap_or_default()
    }

    #[zbus(signal)]
    async fn run_added(emitter: &SignalEmitter<'_>, run: OwnedObjectPath) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn run_removed(emitter: &SignalEmitter<'_>, run: OwnedObjectPath) -> zbus::Result<()>;
}

/// `org.quire.Cua1.Run`.
pub(crate) struct RunObject<S: Drivable, K: Callers> {
    pub supervisor: Arc<Supervisor<S>>,
    pub id: RunId,
    pub who: Arc<K>,
}

impl<S: Drivable, K: Callers> RunObject<S, K> {
    async fn control(&self, member: CuaMember, header: &Header<'_>) -> Result<(), CuaError> {
        let caller = caller_of(&self.who, header).await;
        Ok(self.supervisor.control(caller, &self.id, member).await?)
    }

    fn view(&self) -> fdo::Result<cua_host::RunView> {
        self.supervisor
            .view(&self.id)
            .ok_or_else(|| fdo::Error::UnknownObject("the run is over".into()))
    }
}

#[zbus::interface(name = "org.quire.Cua1.Run")]
impl<S: Drivable, K: Callers> RunObject<S, K> {
    async fn cancel(&self, #[zbus(header)] header: Header<'_>) -> Result<(), CuaError> {
        self.control(CuaMember::Cancel, &header).await
    }

    async fn pause(&self, #[zbus(header)] header: Header<'_>) -> Result<(), CuaError> {
        self.control(CuaMember::Pause, &header).await
    }

    async fn resume(&self, #[zbus(header)] header: Header<'_>) -> Result<(), CuaError> {
        self.control(CuaMember::Resume, &header).await
    }

    async fn answer(
        &self,
        question: u64,
        answer: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), CuaError> {
        let caller = caller_of(&self.who, &header).await;
        // The shell's answer is the person's own words.
        let words = Labelled::new(answer, Label::trusted_user());
        Ok(self
            .supervisor
            .answer(caller, &self.id, QuestionId(question), words)
            .await?)
    }

    async fn take_over(&self, #[zbus(header)] header: Header<'_>) -> Result<(), CuaError> {
        self.control(CuaMember::TakeOver, &header).await
    }

    async fn hand_back(&self, #[zbus(header)] header: Header<'_>) -> Result<(), CuaError> {
        self.control(CuaMember::HandBack, &header).await
    }

    // The `RunId` slug; the path spelling of it is lossy (`-` and `.` become `_`).
    #[zbus(property)]
    async fn id(&self) -> String {
        self.id.as_str().to_owned()
    }

    #[zbus(property)]
    async fn state(&self) -> fdo::Result<String> {
        Ok(slug_of(&self.view()?.phase.slug()).unwrap_or_default())
    }

    #[zbus(property)]
    async fn step(&self) -> fdo::Result<u32> {
        Ok(self.view()?.phase.step().map_or(0, |n| n.0))
    }

    #[zbus(property)]
    async fn goal(&self) -> fdo::Result<String> {
        Ok(self.view()?.task.goal.value)
    }

    #[zbus(property)]
    async fn window(&self) -> fdo::Result<String> {
        let view = self.view()?;
        enveloped(WindowBody {
            app: view.task.app,
            title: WindowTitle(view.title),
        })
    }

    #[zbus(property)]
    async fn mode(&self) -> fdo::Result<String> {
        Ok(slug_of(&self.view()?.task.mode).unwrap_or_default())
    }

    #[zbus(property)]
    async fn budget(&self) -> fdo::Result<String> {
        enveloped(self.view()?.budget)
    }

    #[zbus(signal, name = "StateChanged")]
    async fn phase_changed(emitter: &SignalEmitter<'_>, state: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn step_started(emitter: &SignalEmitter<'_>, step: u32) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn thought_delta(emitter: &SignalEmitter<'_>, step: u32, text: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_proposed(
        emitter: &SignalEmitter<'_>,
        step: u32,
        action: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn action_done(emitter: &SignalEmitter<'_>, step: u32, outcome: &str)
    -> zbus::Result<()>;

    #[zbus(signal)]
    async fn ask_user(emitter: &SignalEmitter<'_>, question: u64, ask: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn finished(emitter: &SignalEmitter<'_>, end: &str) -> zbus::Result<()>;
}

/// The path of the Manager.
pub(crate) const MANAGER_PATH: &str = CUA_PATH;

#[cfg(test)]
mod tests {
    use super::*;
    use cua_desktop::BackendKind;
    use cua_fake::{FakePdp, scene::world};
    use docket_dbus::callers::Caller;
    use zbus::object_server::Interface;

    /// Identifies nobody: the introspection needs the types, not a bus.
    struct NoCallers;

    impl Callers for NoCallers {
        async fn caller_of(&self, _: &str) -> Option<Caller> {
            None
        }
    }

    #[test]
    fn the_served_interfaces_are_the_declared_ones() {
        let supervisor = Arc::new(Supervisor::new(
            world(BackendKind::KwinStock, vec![], FakePdp::allow_all()),
            crate::CuadConfig::default(),
        ));
        let mut served = String::from(
            "<!DOCTYPE node PUBLIC \"-//freedesktop//DTD D-BUS Object Introspection 1.0//EN\"\n \"http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd\">\n<node>\n",
        );
        ManagerObject {
            supervisor: supervisor.clone(),
            who: Arc::new(NoCallers),
        }
        .introspect_to_writer(&mut served, 1);
        RunObject {
            supervisor,
            id: RunId::parse("r-1").unwrap(),
            who: Arc::new(NoCallers),
        }
        .introspect_to_writer(&mut served, 1);
        served.push_str("</node>\n");
        let declared = include_str!("../../../dbus/org.quire.Cua1.xml");
        assert_eq!(served, declared);
    }
}
