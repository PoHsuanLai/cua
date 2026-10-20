//! From an observation to the model's turn.

use crate::observation::{CursorObs, Observation, TreeObs};
use a11y_tree::render;
use cua_bus::{ActionOutcome, ActionResult, LocalRefusal, Refusal};
use docket_core::{InboundLine, InboundPart, Reveal};
use maskcap::{PixelBuf, WholeMask};
use porter_infer::{
    AttachIndex, CuaStepRequest, FrameImage, FrameLayout, ImageSource, MaskedRegions, NoteFrom,
    PrevResult, StepNote, TreeText, WindowGeometry,
};
use prov::{AgentRef, Integrity};
use serde::{Deserialize, Serialize};

/// The frame's bytes, sent beside the request as a memfd attached to the inferd session (never
/// base64 on the bus). The request's `FrameImage` points at it by index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameAttachment(pub PixelBuf);

/// How an earlier action ended, in the words the model is told: a refusal says only that it was
/// refused (the router's reasons are coarse on purpose and are not an oracle to probe), and a
/// failure names what went wrong with the input.
fn prev_result(outcome: &ActionOutcome) -> PrevResult {
    match &outcome.result {
        ActionResult::Done | ActionResult::Answered(_) => PrevResult::Done,
        ActionResult::NotExecuted => PrevResult::NotRun,
        ActionResult::Refused(Refusal::Gate(_)) => PrevResult::Refused("not allowed".to_owned()),
        ActionResult::Refused(Refusal::Local(LocalRefusal::OutsideLease)) => {
            PrevResult::Refused("outside the window".to_owned())
        }
        ActionResult::Refused(Refusal::Local(LocalRefusal::SecretField)) => {
            PrevResult::Refused("a secret field".to_owned())
        }
        ActionResult::Refused(Refusal::Local(LocalRefusal::NotFocused)) => {
            PrevResult::Refused("the window is not focused: click in it first".to_owned())
        }
        ActionResult::Refused(Refusal::Local(LocalRefusal::StateMoved)) => {
            PrevResult::Refused("the window changed".to_owned())
        }
        ActionResult::UserDeclined => PrevResult::UserDeclined,
        ActionResult::Failed(error) => PrevResult::Failed(error.to_string()),
        ActionResult::UserActed { .. } => PrevResult::UserActed,
        ActionResult::CheckFailed => {
            PrevResult::Failed("the success check did not pass".to_owned())
        }
    }
}

/// The note an answer to the model's own question becomes: the person's words, or an agent's
/// when the label does not say they are the person's.
fn answer_note(outcome: &ActionOutcome) -> Option<StepNote> {
    match &outcome.result {
        ActionResult::Answered(answer) => Some(StepNote {
            from: match answer.label.integrity {
                Integrity::Trusted => NoteFrom::Person,
                Integrity::Untrusted => NoteFrom::Agent,
            },
            text: answer.value.clone(),
        }),
        _ => None,
    }
}

/// The note a message that reached the run becomes: its plain text parts, one per line, from
/// the person when the sender is the person and from an agent otherwise. A part the router held
/// back as a handle is not text the run may read, and a message with no plain text says
/// nothing. Pure.
pub fn note_of(line: &InboundLine) -> Option<StepNote> {
    let text: Vec<&str> = line
        .parts
        .iter()
        .filter_map(|part| match part {
            InboundPart::Text(Reveal::Plain(text)) => Some(text.as_str()),
            _ => None,
        })
        .collect();
    if text.is_empty() {
        return None;
    }
    Some(StepNote {
        from: match line.from.agent {
            AgentRef::User => NoteFrom::Person,
            _ => NoteFrom::Agent,
        },
        text: text.join("\n"),
    })
}

/// How many regions were blacked out: each region applied, and the whole frame as one more when
/// it was blacked.
fn masked_regions(obs: &Observation) -> MaskedRegions {
    let whole = match obs.frame.masks.whole {
        WholeMask::None => 0,
        WholeMask::Blacked(_) => 1,
    };
    let count = obs.frame.masks.regions.len().saturating_add(whole);
    MaskedRegions(u16::try_from(count).unwrap_or(u16::MAX))
}

/// The step request for `obs`: the window geometry, the frame described and attached, the cursor,
/// how the previous actions ended, how many regions were masked, and the tree rendered to text
/// (secret values never rendered). `notes` is the messages that reached the run since the last
/// step as `StepNote`s, oldest first (empty when none: an agent's note is untrusted text, a
/// person's is their own words, and neither is authority), then the person's answers to the
/// model's own questions. Pure.
pub fn turn(obs: &Observation) -> (CuaStepRequest, FrameAttachment) {
    let pixels = obs.frame.pixels.clone();
    let tree = match &obs.tree {
        TreeObs::Absent(_) => TreeText::Absent,
        TreeObs::Present(snapshot) => TreeText::Present(render(snapshot)),
    };
    let mut request = CuaStepRequest::new(
        obs.step,
        WindowGeometry {
            logical: obs.window.size,
            scale: obs.window.scale,
        },
        FrameImage {
            source: ImageSource::Attached(AttachIndex(0)),
            layout: FrameLayout::Raw {
                format: pixels.format,
                size: pixels.size,
                stride: pixels.stride,
            },
        },
        tree,
    )
    .with_prev(obs.prev.iter().map(prev_result).collect())
    .with_masked(masked_regions(obs))
    .with_notes(
        obs.notes
            .iter()
            .cloned()
            .chain(obs.prev.iter().filter_map(answer_note))
            .collect(),
    );
    request.cursor = match obs.cursor {
        CursorObs::Unknown => None,
        CursorObs::At(point) => Some(point),
    };
    (request, FrameAttachment(pixels))
}
