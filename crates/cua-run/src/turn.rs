//! From an observation to the model's turn.

use crate::observation::Observation;
use maskcap::PixelBuf;
use porter_infer::CuaStepRequest;
use serde::{Deserialize, Serialize};

/// The frame's bytes, sent beside the request as a memfd attached to the inferd session (never
/// base64 on the bus). The request's `FrameImage` points at it by index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FrameAttachment(pub PixelBuf);

/// The step request for `obs`: the window geometry, the frame described and attached, the cursor,
/// how the previous actions ended, how many regions were masked, and the tree rendered to text
/// (secret values never rendered). Pure.
pub fn turn(obs: &Observation) -> (CuaStepRequest, FrameAttachment) {
    let _ = obs;
    todo!("turn_renders_tree_without_secret_values, turn_notes_masked_regions")
}
