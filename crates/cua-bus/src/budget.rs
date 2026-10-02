//! The limits of one run, and how much of them it has used.

use cua_desktop::Ms;
use porter_core::{Count, MicroUsd};
use serde::{Deserialize, Serialize};

/// What stopped a run that ran out of something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunBudgetKind {
    /// Steps.
    Steps,
    /// Active time (observing through settling; the person's time is not counted).
    Active,
    /// The screen stopped changing.
    Stall,
    /// Actions per minute.
    Rate,
    /// Money.
    Spend,
}

/// The limits of one run. There is no per-run denial count: the router's breaker counts
/// denials across the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunBudget {
    /// Steps (observe, think, act).
    pub steps: Count,
    /// Active wall time.
    pub active: Ms,
    /// Equal frames in a row before the run is called stalled.
    pub stall_frames: Count,
    /// Actions per minute.
    pub actions_per_min: Count,
    /// Failed model calls before the run fails.
    pub model_failures: Count,
    /// Money; zero means local models only.
    pub spend: MicroUsd,
}

impl RunBudget {
    /// The proposed defaults (design/22 `cua.budget.*`): 40 steps, 600 s active, 3 stall
    /// frames, 60 actions a minute, 2 model failures, no spend.
    pub const PROPOSED: RunBudget = RunBudget {
        steps: Count(40),
        active: Ms(600_000),
        stall_frames: Count(3),
        actions_per_min: Count(60),
        model_failures: Count(2),
        spend: MicroUsd(0),
    };
}

/// What a run has used so far (the `Budget` property's `used`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BudgetUse {
    /// Steps taken.
    pub steps: Count,
    /// Active time.
    pub active: Ms,
    /// Equal frames in a row.
    pub stalled: Count,
    /// Actions in the last minute.
    pub actions_last_minute: Count,
    /// Failed model calls.
    pub model_failures: Count,
    /// Money spent.
    pub spend: MicroUsd,
}

impl BudgetUse {
    /// Nothing used yet.
    pub const NONE: BudgetUse = BudgetUse {
        steps: Count(0),
        active: Ms(0),
        stalled: Count(0),
        actions_last_minute: Count(0),
        model_failures: Count(0),
        spend: MicroUsd(0),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_proposed_budget_pins_its_json() {
        let json = serde_json::to_string(&RunBudget::PROPOSED).unwrap();
        assert_eq!(
            json,
            r#"{"steps":40,"active":600000,"stall_frames":3,"actions_per_min":60,"model_failures":2,"spend":0}"#
        );
        assert_eq!(
            serde_json::from_str::<RunBudget>(&json).unwrap(),
            RunBudget::PROPOSED
        );
    }

    #[test]
    fn budget_kinds_and_use_round_trip() {
        let kinds = [
            (RunBudgetKind::Steps, "steps"),
            (RunBudgetKind::Active, "active"),
            (RunBudgetKind::Stall, "stall"),
            (RunBudgetKind::Rate, "rate"),
            (RunBudgetKind::Spend, "spend"),
        ];
        for (kind, slug) in kinds {
            assert_eq!(serde_json::to_string(&kind).unwrap(), format!("\"{slug}\""));
        }
        let json = serde_json::to_string(&BudgetUse::NONE).unwrap();
        assert_eq!(
            serde_json::from_str::<BudgetUse>(&json).unwrap(),
            BudgetUse::NONE
        );
    }
}
