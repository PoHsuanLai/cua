//! A real intentd over the fake router on a private bus, and the records and asks the link tests send it.

use crate::support::bus::PrivateBus;
use cua_action::{CuaAction, WindowSpace};
use cua_action::{RunMode, WindowTrust};
use cua_bus::{
    ActionResult, AuthorisedBy, CuaOutcome, CuaRecord, FrameNote, TargetedAction, ThoughtDigest,
    WindowRef,
};
use cua_desktop::Digest32;
use cua_run::{GateCheck, GateEvent, GateLink};
use cuad::IntentsGate;
use docket_client::{DbusTransport, Intents};
use docket_core::{ConfirmAnswer, GrantScope};
use docket_dbus::callers::{CallerTable, Identity, Role};
use docket_fake::{ScriptedConfirmer, fake_router};
use docket_gate_wire::{CuaAsk, EffectBasis, GateAnswer};
use docket_router::Router;
use docket_wire::{CallerRole, SessionOpen};
use intentd::{AcpGate, IntentdConfig, ProcRoot, serve_on_gated};
use maskcap::FrameDigest;
use porter_core::AppName;
use prov::{
    AgentRef, ConfirmReceipt, Effect, InputProof, Label, RunId, SessionId, Source, SpaceId,
    UnixSeconds,
};
use std::collections::BTreeMap;
use std::sync::Arc;
use zbus::Connection;

pub(crate) const COMPANION: &str = "org.quire.Companion1";
pub(crate) const CUAD: &str = "org.quire.Cua1";

pub(crate) fn mail() -> AppName {
    AppName::parse("org.quire.Mail").unwrap()
}

pub(crate) fn work() -> SpaceId {
    SpaceId::parse("work").unwrap()
}

pub(crate) fn run() -> RunId {
    RunId::parse("r-1").unwrap()
}

pub(crate) fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").unwrap(),
            input: InputProof::HardwareSeat,
            at: UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
        written: None,
    }
}

/// A real intentd over the fake router, on a private bus.
pub(crate) struct Desk {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    who: Arc<Identity>,
    _daemon: Connection,
}

pub(crate) async fn desk(answers: Vec<ConfirmAnswer>) -> Desk {
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
    // Nothing is read from a real `/proc`: the tree is empty and every caller is introduced.
    let proc_root = dir.path().join("proc");
    let who = Arc::new(Identity::on_fixture(
        daemon.clone(),
        CallerTable::default(),
        proc_root.clone(),
    ));
    serve_on_gated(
        &daemon,
        Arc::new(router),
        Arc::new(config),
        who.clone(),
        &ProcRoot::Fake(proc_root),
        AcpGate::shut(),
        None,
    )
    .await
    .expect("intentd serves");
    Desk {
        _dir: dir,
        bus,
        who,
        _daemon: daemon,
    }
}

impl Desk {
    pub(crate) async fn as_companion(&self) -> Intents<DbusTransport> {
        let connection = self.bus.connect().await;
        connection.request_name(COMPANION).await.expect("a name");
        self.who.introduce_app(&connection, COMPANION, Role::Agent);
        // The connection is kept alive by the transport.
        Intents::over(DbusTransport::new(connection))
    }

    pub(crate) async fn cuad_connection(&self) -> Connection {
        let connection = self.bus.connect().await;
        connection.request_name(CUAD).await.expect("a name");
        self.who.introduce_app(&connection, CUAD, Role::Cua);
        connection
    }

    /// What the planner does when it starts a run: opens the run's session, and its own to
    /// hear back on.
    pub(crate) async fn open_sessions(&self, planner: &Intents<DbusTransport>) -> SessionId {
        let hearing = planner
            .session_open(SessionOpen {
                space: work(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                external: None,
                helper: None,
                started_from: None,
                acts_for: None,
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
                helper: None,
                started_from: None,
                acts_for: None,
            })
            .await
            .expect("the run's session")
            .session
    }
}

pub(crate) fn step(effect: Effect) -> CuaAsk {
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

pub(crate) async fn answered(gate: &IntentsGate<DbusTransport>, ask: CuaAsk) -> GateAnswer {
    let mut check = gate.check(ask).await.expect("a check");
    loop {
        match check.next().await.expect("an event") {
            GateEvent::Answered(answer) => return answer,
            // The router is about to ask the person: cuad's input is idle, so say go on.
            GateEvent::Confirming(_) => check.proceed().await.expect("proceed"),
        }
    }
}

pub(crate) fn record_of_run(run: &RunId) -> Vec<CuaRecord> {
    let action = TargetedAction {
        window: WindowRef(1),
        action: CuaAction::<WindowSpace>::Observe,
    };
    vec![
        CuaRecord::RunStarted {
            run: run.clone(),
            session: SessionId::parse("s-2").unwrap(),
            conversation: SessionId::parse("s-1").unwrap(),
            goal: prov::Labelled::new("sign in".to_owned(), Label::trusted_user()),
            app: mail(),
            space: work(),
            mode: RunMode::InPlace,
            backend: cua_desktop::BackendKind::KwinStock,
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
            steps: porter_ids::Count(1),
            charges: porter_infer::ChargeTotal::default(),
            report: None,
        },
    ]
}
