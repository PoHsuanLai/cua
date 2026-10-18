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
    /// The most active time any run may have (two hours), whatever the caller or the settings
    /// ask: a render or an export may take long, a run may not take forever.
    pub const ACTIVE_CEILING: Ms = Ms(7_200_000);

    /// The proposed defaults (design/22 `cua.budget.*`): 40 steps, 1800 s active, 3 stall
    /// frames, 60 actions a minute, 2 model failures, no spend.
    pub const PROPOSED: RunBudget = RunBudget {
        steps: Count(40),
        active: Ms(1_800_000),
        stall_frames: Count(3),
        actions_per_min: Count(60),
        model_failures: Count(2),
        spend: MicroUsd(0),
    };
}

fn lesser<T: PartialOrd + Copy>(a: T, b: T) -> T {
    if a < b { a } else { b }
}

impl RunBudget {
    /// These limits, held to `cap` (the person's own settings): no limit above the cap. A stall
    /// limit of zero would switch stall detection off, so it takes the cap's instead.
    pub fn capped_by(self, cap: RunBudget) -> RunBudget {
        let stall_frames = if self.stall_frames.0 == 0 {
            cap.stall_frames
        } else if cap.stall_frames.0 == 0 {
            self.stall_frames
        } else {
            lesser(self.stall_frames, cap.stall_frames)
        };
        RunBudget {
            steps: lesser(self.steps, cap.steps),
            active: lesser(lesser(self.active, cap.active), Self::ACTIVE_CEILING),
            stall_frames,
            actions_per_min: lesser(self.actions_per_min, cap.actions_per_min),
            model_failures: lesser(self.model_failures, cap.model_failures),
            spend: lesser(self.spend, cap.spend),
        }
    }
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
            r#"{"steps":40,"active":1800000,"stall_frames":3,"actions_per_min":60,"model_failures":2,"spend":0}"#
        );
        assert_eq!(
            serde_json::from_str::<RunBudget>(&json).unwrap(),
            RunBudget::PROPOSED
        );
    }

    #[test]
    fn a_budget_is_held_to_the_cap_and_a_zero_stall_limit_takes_the_caps() {
        let greedy = RunBudget {
            steps: Count(1_000_000),
            active: Ms(u32::MAX),
            stall_frames: Count(0),
            actions_per_min: Count(u32::MAX),
            model_failures: Count(9),
            spend: MicroUsd(u64::MAX),
        };
        assert_eq!(greedy.capped_by(RunBudget::PROPOSED), RunBudget::PROPOSED);
        let modest = RunBudget {
            steps: Count(5),
            ..RunBudget::PROPOSED
        };
        assert_eq!(modest.capped_by(RunBudget::PROPOSED).steps, Count(5));
        let huge_cap = RunBudget {
            active: Ms(u32::MAX),
            ..RunBudget::PROPOSED
        };
        assert_eq!(
            greedy.capped_by(huge_cap).active,
            RunBudget::ACTIVE_CEILING,
            "no run passes the ceiling, whatever the cap allows"
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
