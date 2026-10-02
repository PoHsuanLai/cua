//! The event log and the message path, in memory.

use cua_bus::CuaRecord;
use cua_run::{LinkError, MessageLink, RecordSink};
use docket_core::{Delivery, InboundLine, InboxAsk, MessageDraft};
use prov::{Crossing, MessageId, SessionId, ThreadId};
use std::collections::VecDeque;
use std::sync::Mutex;

/// A [`RecordSink`] and a [`MessageLink`] that keep everything they are given. What a run
/// reports is a `prov::Message` draft of kind `Report`: [`MemoryEvents::sent`] returns them.
#[derive(Debug, Default)]
pub struct MemoryEvents {
    records: Mutex<Vec<CuaRecord>>,
    sent: Mutex<Vec<(SessionId, MessageDraft)>>,
    inbox: Mutex<VecDeque<InboundLine>>,
}

impl MemoryEvents {
    /// Nothing recorded, nothing waiting.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues a message for the run's inbox.
    pub fn deliver(&self, line: InboundLine) {
        self.inbox
            .lock()
            .expect("memory events lock")
            .push_back(line);
    }

    /// The records, in order.
    pub fn records(&self) -> Vec<CuaRecord> {
        self.records.lock().expect("memory events lock").clone()
    }

    /// The reports sent, in order, with the session each was sent for.
    pub fn sent(&self) -> Vec<(SessionId, MessageDraft)> {
        self.sent.lock().expect("memory events lock").clone()
    }
}

impl RecordSink for MemoryEvents {
    async fn record(&self, record: CuaRecord) -> Result<(), LinkError> {
        self.records
            .lock()
            .expect("memory events lock")
            .push(record);
        Ok(())
    }
}

impl MessageLink for MemoryEvents {
    async fn send(&self, session: SessionId, draft: MessageDraft) -> Result<Delivery, LinkError> {
        let mut sent = self.sent.lock().expect("memory events lock");
        let id = MessageId::parse(&format!("m-{}", sent.len() + 1)).expect("a well-formed id");
        let thread = match &draft.thread {
            Some(thread) => thread.clone(),
            None => ThreadId::parse(id.as_str()).expect("a well-formed id"),
        };
        sent.push((session, draft));
        Ok(Delivery {
            message: id,
            thread,
            crossing: Crossing::Within,
        })
    }

    async fn inbox(&self, ask: InboxAsk) -> Result<Vec<InboundLine>, LinkError> {
        let _ = ask;
        Ok(self
            .inbox
            .lock()
            .expect("memory events lock")
            .drain(..)
            .collect())
    }
}
