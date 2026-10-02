//! The phases of a run, and the small facts about them that other crates read.

use cua_bus::{CuaOutcome, QuestionId, RunStateSlug, TargetedAction};
use cua_desktop::Stamp;
use maskcap::Settle;
use porter_infer::StepIndex;
use prov::ConfirmId;
use serde::{Deserialize, Serialize};

/// Where a run is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RunPhase {
    /// Asking the router for the per-app consent.
    Granting,
    /// Asking the desktop for the window.
    Leasing,
    /// Capturing and reading the screen for this step.
    Observing(StepIndex),
    /// Waiting for the model.
    Thinking(StepIndex),
    /// Waiting for the router's answer on the head of the queue.
    Authorising(StepIndex),
    /// The router is asking the person; the lease is suspended on stock.
    Confirming(StepIndex, ConfirmId),
    /// Injecting input.
    Acting(StepIndex),
    /// Waiting for the screen to stop changing.
    Settling(StepIndex, Settle),
    /// Checking the success condition.
    Verifying(StepIndex),
    /// Waiting for the person's answer.
    WaitingForUser(StepIndex, QuestionId),
    /// Paused.
    Paused(StepIndex),
    /// The person has the window, since this instant.
    TakenOver(StepIndex, Stamp),
    /// Over.
    Finished(CuaOutcome),
}

impl RunPhase {
    /// The slug the bus and the log use.
    pub fn slug(&self) -> RunStateSlug {
        match self {
            RunPhase::Granting => RunStateSlug::Granting,
            RunPhase::Leasing => RunStateSlug::Leasing,
            RunPhase::Observing(_) => RunStateSlug::Observing,
            RunPhase::Thinking(_) => RunStateSlug::Thinking,
            RunPhase::Authorising(_) => RunStateSlug::Authorising,
            RunPhase::Confirming(..) => RunStateSlug::Confirming,
            RunPhase::Acting(_) => RunStateSlug::Acting,
            RunPhase::Settling(..) => RunStateSlug::Settling,
            RunPhase::Verifying(_) => RunStateSlug::Verifying,
            RunPhase::WaitingForUser(..) => RunStateSlug::WaitingForUser,
            RunPhase::Paused(_) => RunStateSlug::Paused,
            RunPhase::TakenOver(..) => RunStateSlug::TakenOver,
            RunPhase::Finished(_) => RunStateSlug::Finished,
        }
    }

    /// The step the phase belongs to; none before the first observation and after the end.
    pub fn step(&self) -> Option<StepIndex> {
        match self {
            RunPhase::Granting | RunPhase::Leasing | RunPhase::Finished(_) => None,
            RunPhase::Observing(n)
            | RunPhase::Thinking(n)
            | RunPhase::Authorising(n)
            | RunPhase::Confirming(n, _)
            | RunPhase::Acting(n)
            | RunPhase::Settling(n, _)
            | RunPhase::Verifying(n)
            | RunPhase::WaitingForUser(n, _)
            | RunPhase::Paused(n)
            | RunPhase::TakenOver(n, _) => Some(*n),
        }
    }

    /// Whether wall time in this phase counts against the active budget: observing through
    /// settling, and verifying. Time spent granting, confirming, waiting for the person, paused
    /// or taken over is theirs, not the run's.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            RunPhase::Observing(_)
                | RunPhase::Thinking(_)
                | RunPhase::Authorising(_)
                | RunPhase::Acting(_)
                | RunPhase::Settling(..)
                | RunPhase::Verifying(_)
        )
    }

    /// Whether the run is over.
    pub fn is_finished(&self) -> bool {
        matches!(self, RunPhase::Finished(_))
    }
}

/// The actions of the model's current batch that have not run yet; the head is next. Dropped
/// when the run pauses or an action is refused or fails (the rest are `NotExecuted`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ActionQueue {
    /// The actions, next first.
    pub actions: Vec<TargetedAction>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_bus::CancelledBy;

    #[test]
    fn every_phase_has_its_slug_and_only_the_working_ones_count_as_active() {
        let n = StepIndex(2);
        let confirm = ConfirmId::parse("c-1").unwrap();
        struct Row {
            phase: RunPhase,
            slug: RunStateSlug,
            step: Option<StepIndex>,
            active: bool,
        }
        let rows = [
            Row {
                phase: RunPhase::Granting,
                slug: RunStateSlug::Granting,
                step: None,
                active: false,
            },
            Row {
                phase: RunPhase::Leasing,
                slug: RunStateSlug::Leasing,
                step: None,
                active: false,
            },
            Row {
                phase: RunPhase::Observing(n),
                slug: RunStateSlug::Observing,
                step: Some(n),
                active: true,
            },
            Row {
                phase: RunPhase::Thinking(n),
                slug: RunStateSlug::Thinking,
                step: Some(n),
                active: true,
            },
            Row {
                phase: RunPhase::Authorising(n),
                slug: RunStateSlug::Authorising,
                step: Some(n),
                active: true,
            },
            Row {
                phase: RunPhase::Confirming(n, confirm),
                slug: RunStateSlug::Confirming,
                step: Some(n),
                active: false,
            },
            Row {
                phase: RunPhase::Acting(n),
                slug: RunStateSlug::Acting,
                step: Some(n),
                active: true,
            },
            Row {
                phase: RunPhase::Settling(n, Settle::Quiet),
                slug: RunStateSlug::Settling,
                step: Some(n),
                active: true,
            },
            Row {
                phase: RunPhase::Verifying(n),
                slug: RunStateSlug::Verifying,
                step: Some(n),
                active: true,
            },
            Row {
                phase: RunPhase::WaitingForUser(n, QuestionId(1)),
                slug: RunStateSlug::WaitingForUser,
                step: Some(n),
                active: false,
            },
            Row {
                phase: RunPhase::Paused(n),
                slug: RunStateSlug::Paused,
                step: Some(n),
                active: false,
            },
            Row {
                phase: RunPhase::TakenOver(n, Stamp(5)),
                slug: RunStateSlug::TakenOver,
                step: Some(n),
                active: false,
            },
            Row {
                phase: RunPhase::Finished(CuaOutcome::Cancelled(CancelledBy::User)),
                slug: RunStateSlug::Finished,
                step: None,
                active: false,
            },
        ];
        for row in rows {
            assert_eq!(row.phase.slug(), row.slug, "{:?}", row.phase);
            assert_eq!(row.phase.step(), row.step, "{:?}", row.phase);
            assert_eq!(row.phase.is_active(), row.active, "{:?}", row.phase);
            assert_eq!(row.phase.is_finished(), row.slug == RunStateSlug::Finished);
            let json = serde_json::to_string(&row.phase).unwrap();
            assert_eq!(serde_json::from_str::<RunPhase>(&json).unwrap(), row.phase);
        }
    }

    #[test]
    fn phases_pin_their_json() {
        assert_eq!(
            serde_json::to_string(&RunPhase::Granting).unwrap(),
            r#"{"kind":"granting"}"#
        );
        assert_eq!(
            serde_json::to_string(&RunPhase::Observing(StepIndex(1))).unwrap(),
            r#"{"kind":"observing","v":1}"#
        );
        assert_eq!(
            serde_json::to_string(&RunPhase::Settling(StepIndex(1), Settle::Quiet)).unwrap(),
            r#"{"kind":"settling","v":[1,{"kind":"quiet"}]}"#
        );
    }
}
