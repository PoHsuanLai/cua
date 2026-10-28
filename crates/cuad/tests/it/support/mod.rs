#![allow(dead_code)] // each test binary uses a subset of the shared builders
//! A supervisor over the fakes, and the waits a test needs.

pub mod bus;

use cua_bus::RunEnd;
use cua_desktop::BackendKind;
use cua_fake::scene::{world, world_over};
use cua_fake::{FakePdp, FakeSeams, Fixture};
use cua_run::{CuaConfig, CuaModelError, CuaSignal, Switch};
use cuad::{CuadConfig, Supervisor, SupervisorEvent};
use porter_infer::CuaStepReply;
use prov::RunId;
use std::time::Duration;
use tokio::sync::broadcast::Receiver;

pub fn enabled() -> CuadConfig {
    CuadConfig {
        settings: CuaConfig {
            enabled: Switch::On,
            ..CuaConfig::default()
        },
        ..CuadConfig::default()
    }
}

pub fn supervisor(
    kind: BackendKind,
    script: Vec<Result<CuaStepReply, CuaModelError>>,
    gate: FakePdp,
) -> Supervisor<FakeSeams> {
    Supervisor::new(world(kind, script, gate), enabled())
}

/// [`supervisor`] over another window's accessibility tree.
pub fn supervisor_over(
    kind: BackendKind,
    script: Vec<Result<CuaStepReply, CuaModelError>>,
    gate: FakePdp,
    fixture: Fixture,
) -> Supervisor<FakeSeams> {
    Supervisor::new(world_over(kind, script, gate, fixture), enabled())
}

/// Waits for `run` to end, giving up after a (virtual) minute; the signals it sent on the way
/// come back with how it ended.
pub async fn ended_with(
    events: &mut Receiver<SupervisorEvent>,
    run: &RunId,
) -> (RunEnd, Vec<CuaSignal>) {
    let wait = async {
        let mut signals = Vec::new();
        loop {
            match events.recv().await {
                Ok(SupervisorEvent::Ended(id, end)) if &id == run => return (*end, signals),
                Ok(SupervisorEvent::Signal(id, signal)) if &id == run => signals.push(signal),
                Ok(_) => {}
                Err(error) => panic!("the event stream broke: {error}"),
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(60), wait)
        .await
        .expect("the run ends")
}

/// Waits for `run` to end.
pub async fn ended(events: &mut Receiver<SupervisorEvent>, run: &RunId) -> RunEnd {
    ended_with(events, run).await.0
}

/// Waits until `run` is in a phase `wanted` says yes to, polling its view.
pub async fn until<S: cuad::Drivable>(
    supervisor: &Supervisor<S>,
    run: &RunId,
    wanted: impl Fn(&cua_run::RunPhase) -> bool,
) {
    for _ in 0..2000 {
        if let Some(view) = supervisor.view(run)
            && wanted(&view.phase)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!(
        "the run never reached the phase: {:?}",
        supervisor.view(run)
    );
}
