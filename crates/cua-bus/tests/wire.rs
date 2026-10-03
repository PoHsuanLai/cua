//! Every wire and stored type survives serde, the record kinds are stable slugs, and the bodies
//! pin the JSON that sill and the D-Bus codec read.

use cua_action::{
    Button, ClickCount, Coord, CuaAction, DeviceSize, Point, Summary, Target, WindowSpace,
};
use cua_bus::*;
use cua_desktop::{BackendKind, Digest32, Ms, Stamp, ToplevelIdent, WindowTitle};
use docket_core::{CallRefusal, EffectBasis, RunMode, WindowClass};
use maskcap::FrameDigest;
use porter_core::{AccountId, AppName, Count, DataClass, Locality, MicroUsd, ModelId, UnixSeconds};
use porter_infer::{ServedBy, StepIndex};
use prov::{
    Address, AgentRef, Confidentiality, Effect, Integrity, Label, Labelled, MessageId, RunId,
    SessionId, Source, SpaceId, ThreadId,
};
use seat_input::{InputOrigin, InputReceipt, SeatError, TextPath};
use serde::{Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;

fn space() -> SpaceId {
    SpaceId::parse("desktop").unwrap()
}

fn screen(text: &str) -> Labelled<String> {
    Labelled::new(
        text.to_owned(),
        Label {
            integrity: Integrity::Untrusted,
            confidentiality: Confidentiality::Private(BTreeSet::from([space()])),
            classes: BTreeSet::from([DataClass::Screen]),
            sources: BTreeSet::from([Source::Screen {
                app: AppName::parse("org.mozilla.firefox").unwrap(),
            }]),
        },
    )
}

fn user(text: &str) -> Labelled<String> {
    Labelled::new(
        text.to_owned(),
        Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Public,
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::User]),
        },
    )
}

fn click() -> TargetedAction {
    TargetedAction {
        window: WindowRef(1),
        action: CuaAction::<WindowSpace>::Click {
            at: Target::Point(Point::new(Coord(340), Coord(20))),
            button: Button::Left,
            count: ClickCount::One,
            mods: BTreeSet::new(),
        },
    }
}

fn run() -> RunId {
    RunId::parse("r-17").unwrap()
}

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: &T) -> String {
    let json = serde_json::to_string(value).expect("serialises");
    let back: T = serde_json::from_str(&json).expect("deserialises");
    assert_eq!(&back, value, "{json}");
    json
}

fn every_record() -> Vec<(CuaRecord, &'static str)> {
    let step = StepIndex(3);
    vec![
        (
            CuaRecord::RunStarted {
                run: run(),
                session: SessionId::parse("s-2").unwrap(),
                goal: user("download the March invoice"),
                app: AppName::parse("org.mozilla.firefox").unwrap(),
                space: space(),
                mode: RunMode::InPlace,
                backend: BackendKind::CosmicStock,
                budget: RunBudget::PROPOSED,
            },
            "cua.run.started",
        ),
        (
            CuaRecord::Routed {
                run: run(),
                served_by: ServedBy {
                    account: AccountId::parse("local").unwrap(),
                    model: ModelId::parse("holo-3-1-4b").unwrap(),
                    locality: Locality::OnDevice,
                },
            },
            "cua.run.routed",
        ),
        (
            CuaRecord::Step {
                run: run(),
                n: step,
                thought: ThoughtDigest(Digest32([7; 32])),
                action: click(),
                effect: Effect::UndoableWrite,
                basis: EffectBasis::WindowClass(WindowClass::Ordinary),
                authorised_by: AuthorisedBy::Policy,
                result: ActionResult::Done,
                frame: FrameNote::NotKept {
                    digest: FrameDigest(Digest32([9; 32])),
                    size: DeviceSize { w: 852, h: 532 },
                },
                input: Some(InputReceipt {
                    lease: cua_desktop::LeaseId(1),
                    ops: Count(4),
                    text_path: Some(TextPath::EiKeymap),
                    origin: InputOrigin::SharedSeatViaEi,
                    at: Stamp(9000),
                }),
            },
            "cua.step",
        ),
        (
            CuaRecord::Asked {
                run: run(),
                n: step,
                question_id: QuestionId(2),
                question: screen("Which account?"),
            },
            "cua.asked",
        ),
        (
            CuaRecord::Confirmed {
                run: run(),
                n: step,
                by: ConfirmBy::ShellPrompt,
            },
            "cua.confirmed",
        ),
        (
            CuaRecord::TakenOver {
                run: run(),
                n: step,
            },
            "cua.taken_over",
        ),
        (
            CuaRecord::HandedBack {
                run: run(),
                n: step,
                user_events: vec![],
            },
            "cua.handed_back",
        ),
        (
            CuaRecord::Finished {
                run: run(),
                outcome: CuaOutcome::Done,
                steps: Count(9),
                cost: MicroUsd(0),
                report: Some(MessageId::parse("m-88").unwrap()),
            },
            "cua.run.finished",
        ),
        (
            CuaRecord::ProcedureLearned {
                procedure: ProcedureId("org.mozilla.firefox/download-invoice".into()),
                from: run(),
            },
            "cua.procedure.learned",
        ),
    ]
}

#[test]
fn record_kind_slugs_are_stable_and_every_variant_round_trips() {
    for (record, slug) in every_record() {
        assert_eq!(record.kind(), slug);
        assert_eq!(record.run(), &run());
        let json = round_trip(&record);
        assert!(
            json.starts_with(&format!("{{\"kind\":\"{slug}\",\"v\":{{")),
            "{json}"
        );
    }
}

#[test]
fn a_finished_record_pins_its_json() {
    let (record, _) = every_record().remove(7);
    assert_eq!(
        serde_json::to_string(&record).unwrap(),
        r#"{"kind":"cua.run.finished","v":{"run":"r-17","outcome":{"kind":"done"},"steps":9,"cost":0,"report":"m-88"}}"#
    );
}

#[test]
fn the_actor_of_a_run_is_a_cua_actor_that_maps_to_the_cua_roster_party() {
    let actor = cua_actor(SessionId::parse("s-2").unwrap(), run());
    assert_eq!(actor.kind(), prov::ActorKind::Cua);
    assert_eq!(AgentRef::of(&actor), Some(AgentRef::Cua { run: run() }));
}

#[test]
fn action_results_pin_their_json() {
    let table: Vec<(ActionResult, &str)> = vec![
        (ActionResult::Done, r#"{"kind":"done"}"#),
        (ActionResult::NotExecuted, r#"{"kind":"not_executed"}"#),
        (
            ActionResult::Refused(Refusal::Gate(CallRefusal::Timeout)),
            r#"{"kind":"refused","v":{"kind":"gate","v":{"kind":"timeout"}}}"#,
        ),
        (
            ActionResult::Refused(Refusal::Local(LocalRefusal::SecretField)),
            r#"{"kind":"refused","v":{"kind":"local","v":"secret_field"}}"#,
        ),
        (ActionResult::UserDeclined, r#"{"kind":"user_declined"}"#),
        (
            ActionResult::Failed(SeatError::Suspended),
            r#"{"kind":"failed","v":{"kind":"suspended"}}"#,
        ),
        (
            ActionResult::UserActed { since: Stamp(120) },
            r#"{"kind":"user_acted","v":{"since":120}}"#,
        ),
        (ActionResult::CheckFailed, r#"{"kind":"check_failed"}"#),
    ];
    for (result, json) in table {
        assert_eq!(serde_json::to_string(&result).unwrap(), json);
        assert_eq!(serde_json::from_str::<ActionResult>(json).unwrap(), result);
    }
    let answered = ActionResult::Answered(user("the work account"));
    round_trip(&answered);
    round_trip(&ActionOutcome {
        action: click(),
        result: answered,
    });
}

#[test]
fn a_task_round_trips_with_its_report_route() {
    let task = CuaTask {
        app: AppName::parse("org.mozilla.firefox").unwrap(),
        goal: user("download the March invoice"),
        target: TaskTarget::App(AppName::parse("org.mozilla.firefox").unwrap()),
        mode: RunMode::InPlace,
        space: space(),
        session: SessionId::parse("s-2").unwrap(),
        budget: RunBudget::PROPOSED,
        data_in: vec![],
        success: SuccessCheck::ModelSays,
        origin: RunOrigin::Planner,
        procedure: ProcedureUse::None,
        report: ReportRoute {
            to: Address::new(AgentRef::Companion, space()),
            thread: Some(ThreadId::parse("t-7").unwrap()),
            in_reply_to: Some(MessageId::parse("m-41").unwrap()),
        },
    };
    let json = round_trip(&CuaEnvelope::current(task.clone()));
    assert!(
        json.starts_with(r#"{"vocab":1,"body":{"app":"org.mozilla.firefox","goal":"#),
        "{json}"
    );
    let window = CuaTask {
        target: TaskTarget::Window(ToplevelIdent::parse("w1").unwrap()),
        origin: RunOrigin::Handover { since: Stamp(5) },
        procedure: ProcedureUse::Hint(ProcedureId("p".into())),
        ..task
    };
    round_trip(&window);
}

#[test]
fn the_property_and_signal_bodies_round_trip() {
    round_trip(&WindowBody {
        app: AppName::parse("org.mozilla.firefox").unwrap(),
        title: WindowTitle("Invoices".into()),
    });
    let budget = BudgetBody {
        limits: RunBudget::PROPOSED,
        used: BudgetUse::NONE,
    };
    let json = round_trip(&budget);
    assert!(json.starts_with(r#"{"limits":{"steps":40"#), "{json}");
    round_trip(&AskBody {
        question: screen("Which?"),
        choices: vec![screen("Work"), screen("Home")],
    });
    round_trip(&click());
}

#[test]
fn the_report_of_a_finished_run_is_a_report_message_whatever_the_outcome() {
    let route = ReportRoute {
        to: Address::new(AgentRef::Companion, space()),
        thread: None,
        in_reply_to: None,
    };
    let outcomes = [
        CuaOutcome::Done,
        CuaOutcome::Failed(FailReason::WindowGone),
        CuaOutcome::Blocked(BlockedOn::ConsentDenied),
        CuaOutcome::Cancelled(CancelledBy::User),
        CuaOutcome::BudgetOut(RunBudgetKind::Steps),
    ];
    for outcome in outcomes {
        let draft = report_draft(&route, outcome.status(), &Summary::new("").unwrap(), &[]);
        assert_eq!(
            draft.kind,
            prov::MessageKind::Report {
                status: outcome.status()
            },
            "{outcome:?}"
        );
    }
    let _unused = (Ms(0), UnixSeconds(0));
}

#[test]
fn a_handed_back_record_carries_the_events_the_person_made_and_pins_its_json() {
    use almanac_core::{EventRef, ReplicaId, Seq};
    let events = vec![
        EventRef {
            space: space(),
            replica: ReplicaId([7; 16]),
            seq: Seq(41),
        },
        EventRef {
            space: space(),
            replica: ReplicaId([7; 16]),
            seq: Seq(42),
        },
    ];
    let record = CuaRecord::HandedBack {
        run: run(),
        n: porter_infer::StepIndex(3),
        user_events: events.clone(),
    };
    let json = round_trip(&record);
    assert!(
        json.contains(r#""user_events":[{"space":"desktop","replica":"#),
        "{json}"
    );
    assert!(
        json.contains(r#""seq":41"#) && json.contains(r#""seq":42"#),
        "{json}"
    );
    let back: CuaRecord = serde_json::from_str(&json).unwrap();
    let CuaRecord::HandedBack { user_events, .. } = back else {
        panic!("not a hand-back")
    };
    assert_eq!(user_events, events, "oldest first, as given");
}
