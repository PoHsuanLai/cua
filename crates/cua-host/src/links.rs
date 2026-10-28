//! The event log and the message path over the daemons: memoryd for records, the router for
//! reports.

use crate::clock::SystemClock;
use crate::lock::locked;
use almanac_client::{Memory, Transport as MemoryTransport};
use almanac_core::{
    AreaPayload, AreaTag, Cause, EventBody, JsonText, KindTag, Record, ThingRole, ThingView,
    UserText,
};
use cua_bus::{CuaRecord, CuaTask};
use cua_run::Clock;
use cua_run::{LinkError, MessageLink, RecordSink};
use docket_client::{ClientError, Intents, Transport as IntentsTransport, TransportError};
use docket_core::{Delivery, InboundLine, InboxAsk, MessageDraft};
use porter_core::{AppName, DataClass};
use porter_ids::UnixSeconds;
use prov::{Effect, EntityId, EntityKey, EntityKind, Label, RunId, SessionId, Source, SpaceId};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The app whose things the conversations are. Must match docket's session log
/// (`docket-memory`'s `SESSION_APP`): the router reads a conversation's records by this thing.
const CONVERSATION_APP: &str = "org.quire.Companion";
/// The kind of the thing a conversation names itself as. Must match docket's session log
/// (`docket-memory`'s `SESSION_KIND`).
const CONVERSATION_KIND: &str = "companion.session";
/// The kind of the thing an app is, when a record names the app the run drives.
const APP_KIND: &str = "desktop.app";

/// What a record needs besides itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordContext {
    /// The session the run acts for.
    pub session: SessionId,
    /// The conversation the run works for; every record names it as its `Subject`.
    pub conversation: SessionId,
    /// The app of the leased window; every record names it as its `Source`.
    pub app: AppName,
    /// The Space.
    pub space: SpaceId,
    /// The run's taint: `Untrusted(Screen)` once it has seen the window.
    pub label: Label,
    /// When it happened.
    pub at: UnixSeconds,
}

/// Why a record could not become an event.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RecordError {
    /// The record's slug is not a kind tag (a bug: the slugs are pinned by a test).
    #[error("not a kind tag: {0}")]
    Kind(String),
    /// The record did not serialise.
    #[error("not JSON: {0}")]
    Json(String),
    /// The conversation or the app is not a thing (a bug: both are validated ids).
    #[error("not a thing: {0}")]
    Thing(String),
}

/// A thing as an event names it, with no title: the log keeps only who it is.
fn view_of(app: &str, kind: &str, key: &str) -> Result<ThingView, RecordError> {
    let thing = |e: String| RecordError::Thing(e);
    Ok(ThingView {
        thing: EntityId {
            app: AppName::parse(app).map_err(|e| thing(e.to_string()))?,
            kind: EntityKind::parse(kind).map_err(|e| thing(e.to_string()))?,
            key: EntityKey::parse(key).map_err(|e| thing(e.to_string()))?,
        },
        title: UserText::new(""),
        subtitle: UserText::new(""),
    })
}

/// The things every record of a run names: the conversation (`Subject`, exactly as docket's
/// session log names it) and the app the run drives (`Source`; almanac has no `Object` role).
fn things_of(ctx: &RecordContext) -> Result<Vec<(ThingView, ThingRole)>, RecordError> {
    Ok(vec![
        (
            view_of(
                CONVERSATION_APP,
                CONVERSATION_KIND,
                ctx.conversation.as_str(),
            )?,
            ThingRole::Subject,
        ),
        (
            view_of(ctx.app.as_str(), APP_KIND, ctx.app.as_str())?,
            ThingRole::Source,
        ),
    ])
}

/// The almanac event for `record`: an `EventBody::Area` of area `Cua` whose JSON is the record's
/// serde form and whose kind tag is its slug, by the actor `Companion { session, role: Cua }`.
/// Pure. A step's effect is the record's own; every other record is a `Read`.
pub fn record_of(ctx: &RecordContext, record: &CuaRecord) -> Result<Record, RecordError> {
    let kind = KindTag::parse(&record.kind()).map_err(|e| RecordError::Kind(e.to_string()))?;
    let json = serde_json::to_string(record).map_err(|e| RecordError::Json(e.to_string()))?;
    let json = JsonText::parse(&json).map_err(|e| RecordError::Json(e.to_string()))?;
    let effect = match record {
        CuaRecord::Step { effect, .. } => *effect,
        _ => Effect::Read,
    };
    let things = things_of(ctx)?;
    Ok(Record {
        space: ctx.space.clone(),
        occurred: ctx.at,
        actor: cua_bus::cua_actor(ctx.session.clone(), record.run().clone()),
        effect,
        label: ctx.label.clone(),
        body: EventBody::Area(AreaPayload {
            area: AreaTag::Cua,
            kind,
            json,
            things,
        }),
        cause: Cause::None,
    })
}

/// The seconds since the epoch: the system's clock, here in the daemon.
fn system_unix() -> UnixSeconds {
    SystemClock::start().unix()
}

/// Records over memoryd and messages over the router.
///
/// A record carries only its run, so the links learn what a record needs besides itself (the
/// session, the Space, the taint of what the run read) when the run opens (`RecordSink::open`)
/// and keep it until its `Finished`.
#[derive(Debug)]
pub struct DaemonLinks<M: MemoryTransport, I: IntentsTransport> {
    memory: Memory<M>,
    intents: Intents<I>,
    runs: Mutex<BTreeMap<RunId, RecordContext>>,
    now: fn() -> UnixSeconds,
}

impl<M: MemoryTransport, I: IntentsTransport> DaemonLinks<M, I> {
    /// Links over a memoryd client and a router client.
    pub fn over(memory: Memory<M>, intents: Intents<I>) -> Self {
        Self {
            memory,
            intents,
            runs: Mutex::new(BTreeMap::new()),
            now: system_unix,
        }
    }

    /// The same links reading wall time from `now` (tests pin it).
    pub fn reading_time_from(self, now: fn() -> UnixSeconds) -> Self {
        Self { now, ..self }
    }

    /// What `record` needs besides the record: what the run's start taught the links.
    fn context_of(&self, record: &CuaRecord) -> Option<RecordContext> {
        let mut runs = locked(&self.runs);
        let context = runs.get(record.run()).map(|known| RecordContext {
            at: (self.now)(),
            ..known.clone()
        });
        if let CuaRecord::Finished { run, .. } = record {
            runs.remove(run);
        }
        context
    }
}

/// What memoryd's refusal or absence means to a run.
fn memory_error(error: almanac_client::ClientError) -> LinkError {
    match error {
        almanac_client::ClientError::Transport(
            almanac_client::TransportError::Closed | almanac_client::TransportError::Absent,
        ) => LinkError::Unavailable,
        other => LinkError::Protocol(other.to_string()),
    }
}

/// What the router's answer means to a run.
fn router_error(error: ClientError) -> LinkError {
    match error {
        ClientError::Transport(TransportError::Closed) => LinkError::Unavailable,
        ClientError::Transport(other) => LinkError::Protocol(other.to_string()),
        ClientError::Refused(refusal) => LinkError::Refused(refusal),
        // Unexpected, or a failure a later docket adds: the run stops, never proceeds.
        _ => LinkError::Protocol("unexpected reply".to_owned()),
    }
}

impl<M: MemoryTransport, I: IntentsTransport> RecordSink for DaemonLinks<M, I> {
    async fn open(&self, run: &RunId, task: &CuaTask) {
        let label = Label::untrusted(
            Source::Screen {
                app: task.app.clone(),
            },
            DataClass::Screen,
            task.space.clone(),
        );
        locked(&self.runs).insert(
            run.clone(),
            RecordContext {
                session: task.session.clone(),
                conversation: task.conversation.clone(),
                app: task.app.clone(),
                space: task.space.clone(),
                label,
                at: (self.now)(),
            },
        );
    }

    async fn record(&self, record: CuaRecord) -> Result<(), LinkError> {
        let Some(context) = self.context_of(&record) else {
            return Err(LinkError::Protocol(format!(
                "no run to record {} for",
                record.kind()
            )));
        };
        let event =
            record_of(&context, &record).map_err(|error| LinkError::Protocol(error.to_string()))?;
        self.memory
            .record(event)
            .await
            .map(|_| ())
            .map_err(memory_error)
    }
}

impl<M: MemoryTransport, I: IntentsTransport> MessageLink for DaemonLinks<M, I> {
    async fn send(&self, session: SessionId, draft: MessageDraft) -> Result<Delivery, LinkError> {
        self.intents
            .send(session, draft)
            .await
            .map_err(router_error)
    }

    async fn inbox(&self, ask: InboxAsk) -> Result<Vec<InboundLine>, LinkError> {
        self.intents.inbox(ask).await.map_err(router_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_bus::{CancelledBy, CuaOutcome};
    use porter_core::DataClass;
    use porter_ids::Count;
    use porter_infer::ChargeTotal;
    use prov::{ActorKind, Confidentiality, Integrity, RunId, Source};
    use std::collections::BTreeSet;

    fn ctx() -> RecordContext {
        let space = SpaceId::parse("desktop").unwrap();
        RecordContext {
            session: SessionId::parse("s-1").unwrap(),
            conversation: SessionId::parse("c-1").unwrap(),
            app: porter_core::AppName::parse("org.mozilla.firefox").unwrap(),
            space: space.clone(),
            label: Label {
                integrity: Integrity::Untrusted,
                confidentiality: Confidentiality::Private(BTreeSet::from([space])),
                classes: BTreeSet::from([DataClass::Screen]),
                sources: BTreeSet::from([Source::Screen {
                    app: porter_core::AppName::parse("org.mozilla.firefox").unwrap(),
                }]),
            },
            at: UnixSeconds(1_700_000_000),
        }
    }

    #[test]
    fn a_record_becomes_an_area_cua_event_by_the_cua_actor() {
        let record = CuaRecord::Finished {
            run: RunId::parse("r-1").unwrap(),
            outcome: CuaOutcome::Cancelled(CancelledBy::User),
            steps: Count(3),
            charges: ChargeTotal::default(),
            report: None,
        };
        let event = record_of(&ctx(), &record).expect("an event");
        assert_eq!(event.actor.kind(), ActorKind::Cua);
        assert_eq!(event.effect, Effect::Read);
        assert_eq!(event.label, ctx().label);
        let EventBody::Area(payload) = &event.body else {
            panic!("not an area body")
        };
        assert_eq!(payload.area, AreaTag::Cua);
        assert_eq!(payload.kind.as_str(), "cua.run.finished");
        assert_eq!(
            payload.json.as_str(),
            serde_json::to_string(&record).unwrap()
        );
        let named: Vec<(String, String, String, ThingRole)> = payload
            .things
            .iter()
            .map(|(view, role)| {
                (
                    view.thing.app.as_str().to_owned(),
                    view.thing.kind.as_str().to_owned(),
                    view.thing.key.as_str().to_owned(),
                    *role,
                )
            })
            .collect();
        let expected = |app: &str, kind: &str, key: &str, role| {
            (app.to_owned(), kind.to_owned(), key.to_owned(), role)
        };
        assert_eq!(
            named,
            vec![
                expected(
                    "org.quire.Companion",
                    "companion.session",
                    "c-1",
                    ThingRole::Subject
                ),
                expected(
                    "org.mozilla.firefox",
                    "desktop.app",
                    "org.mozilla.firefox",
                    ThingRole::Source
                ),
            ]
        );
    }
}
