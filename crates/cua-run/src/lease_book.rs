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
    /// already holds a window.
    pub fn claim(&self, window: &ToplevelIdent, run: &RunId) -> Result<LeaseBook, LeaseHeld> {
        let _ = (window, run);
        todo!("lease_book_one_run_per_window")
    }

    /// The book after the holder's input is stopped or restarted.
    pub fn set_state(&self, run: &RunId, state: SlotState) -> LeaseBook {
        let _ = (run, state);
        todo!("lease_book_one_run_per_window")
    }

    /// The book after `run` lets go of its window.
    pub fn release(&self, run: &RunId) -> LeaseBook {
        let _ = run;
        todo!("lease_book_one_run_per_window")
    }

    /// The run holding `window`, if any.
    pub fn holder(&self, window: &ToplevelIdent) -> Option<&RunId> {
        self.slots.get(window).map(|slot| &slot.run)
    }
}
