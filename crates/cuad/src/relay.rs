//! The supervisor's events on the bus: the run objects and the signals the shell hears.

use crate::bus::{MANAGER_PATH, RunObject, enveloped};
use crate::callers::shell_owner;
use cua_bus::AskBody;
use cua_dbus::run_path;
use cua_host::Drivable;
use cua_host::Supervisor;
use cua_host::SupervisorEvent;
use cua_run::CuaSignal;
use docket_dbus::callers::Identity;
use prov::RunId;
use std::collections::BTreeSet;
use std::sync::Arc;
use tokio::sync::broadcast;
use zbus::zvariant::OwnedObjectPath;

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
pub(crate) async fn relay<S: Drivable>(
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
