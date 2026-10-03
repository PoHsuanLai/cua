//! Which run holds which window: one run per window, one window per run.

use cua_desktop::ToplevelIdent;
use prov::RunId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Whether the holder is acting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlotState {
    /// Acting.
    Held,
    /// Input stopped (paused, taken over, or a confirmation showing); still the holder's.
    Suspended,
}

/// One held window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
    /// The run.
    pub run: RunId,
    /// Whether it is acting.
    pub state: SlotState,
}

/// Another run holds the window, or the run holds another window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("the lease is held by {by}")]
pub struct LeaseHeld {
    /// The run in the way.
    pub by: RunId,
}

/// The book: `Free -> Held(run) -> Suspended(run) -> Free` per window. Values in, values out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LeaseBook {
    /// The held windows.
    pub slots: BTreeMap<ToplevelIdent, Slot>,
}

impl LeaseBook {
    /// The book after `run` takes `window`; refused when another run holds the window or `run`
    /// already holds a window (the holder in the way is named).
    pub fn claim(&self, window: &ToplevelIdent, run: &RunId) -> Result<LeaseBook, LeaseHeld> {
        if let Some(slot) = self.slots.get(window) {
            return Err(LeaseHeld {
                by: slot.run.clone(),
            });
        }
        if let Some(slot) = self.slots.values().find(|slot| slot.run == *run) {
            return Err(LeaseHeld {
                by: slot.run.clone(),
            });
        }
        let mut slots = self.slots.clone();
        slots.insert(
            window.clone(),
            Slot {
                run: run.clone(),
                state: SlotState::Held,
            },
        );
        Ok(LeaseBook { slots })
    }

    /// The book after the holder's input is stopped or restarted. A run that holds nothing
    /// changes nothing.
    pub fn set_state(&self, run: &RunId, state: SlotState) -> LeaseBook {
        let slots = self
            .slots
            .iter()
            .map(|(window, slot)| {
                let slot = if slot.run == *run {
                    Slot {
                        run: slot.run.clone(),
                        state,
                    }
                } else {
                    slot.clone()
                };
                (window.clone(), slot)
            })
            .collect();
        LeaseBook { slots }
    }

    /// The book after `run` lets go of its window.
    pub fn release(&self, run: &RunId) -> LeaseBook {
        LeaseBook {
            slots: self
                .slots
                .iter()
                .filter(|(_, slot)| slot.run != *run)
                .map(|(window, slot)| (window.clone(), slot.clone()))
                .collect(),
        }
    }

    /// The run holding `window`, if any.
    pub fn holder(&self, window: &ToplevelIdent) -> Option<&RunId> {
        self.slots.get(window).map(|slot| &slot.run)
    }
}
