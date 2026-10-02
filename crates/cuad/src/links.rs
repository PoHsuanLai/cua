//! The event log and the message path over the daemons: memoryd for records, the router for
//! reports.

use almanac_client::{Memory, Transport as MemoryTransport};
use almanac_core::{AreaPayload, AreaTag, Cause, EventBody, JsonText, KindTag, Record};
use cua_bus::CuaRecord;
use cua_run::{LinkError, MessageLink, RecordSink};
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{Delivery, InboundLine, InboxAsk, MessageDraft};
use porter_core::UnixSeconds;
use prov::{Effect, Label, SessionId, SpaceId};

/// What a record needs besides itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordContext {
    /// The session the run acts for.
    pub session: SessionId,
    /// The Space.
    pub space: SpaceId,
    /// The run's taint: `Untrusted(Screen)` once it has seen the window.
    pub label: Label,
    /// When it happened.
    pub at: UnixSeconds,
}

/// Why a record could not become an event.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordError {
    /// The record's slug is not a kind tag (a bug: the slugs are pinned by a test).
    #[error("not a kind tag: {0}")]
    Kind(String),
    /// The record did not serialise.
    #[error("not JSON: {0}")]
    Json(String),
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
            things: Vec::new(),
        }),
        cause: Cause::None,
    })
}

/// Records over memoryd and messages over the router.
#[derive(Debug)]
pub struct DaemonLinks<M: MemoryTransport, I: IntentsTransport> {
    memory: Memory<M>,
    intents: Intents<I>,
}

impl<M: MemoryTransport, I: IntentsTransport> DaemonLinks<M, I> {
    /// Links over a memoryd client and a router client.
    pub fn over(memory: Memory<M>, intents: Intents<I>) -> Self {
        Self { memory, intents }
    }
}

impl<M: MemoryTransport, I: IntentsTransport> RecordSink for DaemonLinks<M, I> {
    async fn record(&self, record: CuaRecord) -> Result<(), LinkError> {
        let _ = (record, &self.memory);
        todo!(
            "record_of with the run's RecordContext, then Memory::record (Caller::Cuad may record Area(Cua) only)"
        )
    }
}

impl<M: MemoryTransport, I: IntentsTransport> MessageLink for DaemonLinks<M, I> {
    async fn send(&self, session: SessionId, draft: MessageDraft) -> Result<Delivery, LinkError> {
        let _ = (session, draft, &self.intents);
        todo!(
            "Intents::send; a refusal is a LinkError the machine reports as ReportDelivery::Refused"
        )
    }

    async fn inbox(&self, ask: InboxAsk) -> Result<Vec<InboundLine>, LinkError> {
        let _ = ask;
        todo!("Intents::inbox for AgentRef::Cua for the run")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_bus::{CancelledBy, CuaOutcome};
    use porter_core::{Count, DataClass, MicroUsd};
    use prov::{ActorKind, Confidentiality, Integrity, RunId, Source};
    use std::collections::BTreeSet;

    fn ctx() -> RecordContext {
        let space = SpaceId::parse("desktop").unwrap();
        RecordContext {
            session: SessionId::parse("s-1").unwrap(),
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
            cost: MicroUsd(0),
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
        assert!(payload.things.is_empty());
    }
}
