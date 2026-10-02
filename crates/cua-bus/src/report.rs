//! A run speaks as a `prov::Message` of kind `Report`: Done, Failed, Cancelled or Progress.
//! There is no separate result type.
//!
//! What a run hands the router is a [`MessageDraft`]; the router stamps the id, the sender
//! (`AgentRef::Cua { run }`), the label and the time. The label must include
//! `Untrusted(Screen { app })` for everything the run read off the screen: the summary and
//! every extracted value are screen text (FINDINGS, upstream asks).

use crate::state::CuaOutcome;
use cua_action::{Extracted, Summary};
use cua_desktop::Ms;
use docket_core::{Delivery, DraftPart, MessageDraft, SendRefusal};
use porter_core::{Count, MicroUsd};
use prov::{MAX_PARTS, MAX_TEXT_BYTES, MessageKind, MessageText, ReportStatus};
use serde::{Deserialize, Serialize};

use crate::task::ReportRoute;

/// Whether a report reached the router.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ReportDelivery {
    /// Delivered: its message and thread.
    Sent(Delivery),
    /// The router refused it.
    Refused(SendRefusal),
}

/// The `Finished` signal's body: how the run ended, and which message carries its report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunEnd {
    /// How it ended.
    pub outcome: CuaOutcome,
    /// Steps taken.
    pub steps: Count,
    /// Active time used.
    pub active: Ms,
    /// Money spent.
    pub cost: MicroUsd,
    /// The report to the requester.
    pub report: ReportDelivery,
}

/// The text that stands in when a report has nothing to say.
fn fallback(status: ReportStatus) -> &'static str {
    match status {
        ReportStatus::Done => "Done.",
        ReportStatus::Failed => "Could not finish.",
        ReportStatus::Cancelled => "Stopped.",
        ReportStatus::Progress => "Working.",
    }
}

fn clipped(text: &str) -> MessageText {
    let mut end = text.len().min(MAX_TEXT_BYTES);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    MessageText::new(&text[..end])
}

/// The draft of a report: the summary, then one `name: value` line per extracted value, in
/// order. A text longer than a message may hold is cut at a character boundary; more values than
/// a message may hold are dropped from the end; a report with nothing to say still carries one
/// short line, so it is never an empty message. Pure.
pub fn report_draft(
    route: &ReportRoute,
    status: ReportStatus,
    summary: &Summary,
    extracted: &[Extracted],
) -> MessageDraft {
    let first = if summary.as_str().is_empty() {
        fallback(status)
    } else {
        summary.as_str()
    };
    let parts = std::iter::once(clipped(first))
        .chain(
            extracted
                .iter()
                .map(|e| clipped(&format!("{}: {}", e.name.as_str(), e.value.as_str()))),
        )
        .take(MAX_PARTS)
        .map(DraftPart::Text)
        .collect();
    MessageDraft {
        to: route.to.clone(),
        thread: route.thread.clone(),
        in_reply_to: route.in_reply_to.clone(),
        kind: MessageKind::Report { status },
        parts,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_action::{ExtractedText, FieldName};
    use prov::{Address, AgentRef, MessageId, SpaceId, ThreadId};

    fn route() -> ReportRoute {
        ReportRoute {
            to: Address::new(AgentRef::Companion, SpaceId::parse("desktop").unwrap()),
            thread: Some(ThreadId::parse("t-7").unwrap()),
            in_reply_to: Some(MessageId::parse("m-41").unwrap()),
        }
    }

    fn extracted(name: &str, value: &str) -> Extracted {
        Extracted {
            name: FieldName::new(name).unwrap(),
            value: ExtractedText::new(value).unwrap(),
        }
    }

    fn texts(draft: &MessageDraft) -> Vec<String> {
        draft
            .parts
            .iter()
            .map(|p| match p {
                DraftPart::Text(t) => t.as_str().to_owned(),
                other => panic!("not text: {other:?}"),
            })
            .collect()
    }

    #[test]
    fn a_report_is_a_message_draft_of_kind_report_to_the_requester() {
        let draft = report_draft(
            &route(),
            ReportStatus::Done,
            &Summary::new("Downloaded the March invoice.").unwrap(),
            &[
                extracted("invoice_no", "2291"),
                extracted("total", "EUR 118.40"),
            ],
        );
        assert_eq!(
            draft.kind,
            MessageKind::Report {
                status: ReportStatus::Done
            }
        );
        assert_eq!(draft.to, route().to);
        assert_eq!(draft.thread, route().thread);
        assert_eq!(draft.in_reply_to, route().in_reply_to);
        assert_eq!(
            texts(&draft),
            [
                "Downloaded the March invoice.",
                "invoice_no: 2291",
                "total: EUR 118.40"
            ]
        );
    }

    #[test]
    fn an_empty_summary_still_says_one_short_line_per_status() {
        let table = [
            (ReportStatus::Done, "Done."),
            (ReportStatus::Failed, "Could not finish."),
            (ReportStatus::Cancelled, "Stopped."),
            (ReportStatus::Progress, "Working."),
        ];
        for (status, line) in table {
            let draft = report_draft(&route(), status, &Summary::new("").unwrap(), &[]);
            assert_eq!(texts(&draft), [line], "{status:?}");
            assert_eq!(draft.kind, MessageKind::Report { status });
        }
    }

    #[test]
    fn a_report_never_exceeds_what_a_message_may_hold() {
        let many: Vec<Extracted> = (0..80).map(|i| extracted("n", &i.to_string())).collect();
        let draft = report_draft(
            &route(),
            ReportStatus::Done,
            &Summary::new("ok").unwrap(),
            &many,
        );
        assert_eq!(draft.parts.len(), MAX_PARTS);
        assert_eq!(texts(&draft)[1], "n: 0");
        let wide = "😀".repeat(ExtractedText::MAX_CHARS as usize);
        assert!(
            wide.len() > MAX_TEXT_BYTES,
            "the cut is reachable with four-byte characters"
        );
        let draft = report_draft(
            &route(),
            ReportStatus::Done,
            &Summary::new("ok").unwrap(),
            &[extracted("page", &wide)],
        );
        let page = &texts(&draft)[1];
        assert!(page.len() <= MAX_TEXT_BYTES);
        assert!(page.starts_with("page: 😀"));
        assert!(
            page.len() > MAX_TEXT_BYTES - 4,
            "cut at the last whole character, not earlier"
        );
    }

    #[test]
    fn run_end_and_delivery_pin_their_json() {
        let end = RunEnd {
            outcome: CuaOutcome::Done,
            steps: Count(9),
            active: Ms(41_000),
            cost: MicroUsd(0),
            report: ReportDelivery::Refused(SendRefusal::Halted),
        };
        let json = serde_json::to_string(&end).unwrap();
        assert_eq!(
            json,
            r#"{"outcome":{"kind":"done"},"steps":9,"active":41000,"cost":0,"report":{"kind":"refused","v":{"kind":"halted"}}}"#
        );
        assert_eq!(serde_json::from_str::<RunEnd>(&json).unwrap(), end);
    }
}
