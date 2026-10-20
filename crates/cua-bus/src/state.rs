//! Where a run is, how it ended, and what each run mode needs of the backend.

use crate::budget::RunBudgetKind;
use crate::screen::ScreenKind;
use cua_action::RunMode;
use cua_desktop::BackendKind;
use prov::ReportStatus;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The phase of a run as the bus and the event log name it (the `State` property and the
/// `StateChanged` signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStateSlug {
    /// Asking for the per-app consent.
    Granting,
    /// Asking the desktop for the window.
    Leasing,
    /// Capturing and reading the screen.
    Observing,
    /// Waiting for the model.
    Thinking,
    /// Asking the router whether the next action may run.
    Authorising,
    /// A confirmation is showing.
    Confirming,
    /// Injecting input.
    Acting,
    /// Waiting for the screen to stop changing.
    Settling,
    /// Checking the success condition.
    Verifying,
    /// Waiting for the person's answer to a question.
    WaitingForUser,
    /// Paused.
    Paused,
    /// The person has taken over the window.
    TakenOver,
    /// Over.
    Finished,
}

/// Why a run failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FailReason {
    /// The window closed.
    WindowGone,
    /// The model failed too often or fatally.
    Model,
    /// The capture failed twice.
    Capture,
    /// The input could not be delivered.
    Input,
    /// The success check failed and the budget ended.
    CheckFailed,
    /// The model said the task cannot be done.
    Infeasible,
    /// The run's log could not be written after input went out: with no record of what was
    /// done the run does not go on.
    Log,
    /// No model that reads images could be used: the one the person named does not, or none
    /// here does. The run row offers to choose another.
    CannotSeeScreen,
}

/// What stopped a run before it began or kept it from going on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BlockedOn {
    /// The person refused the per-app consent.
    ConsentDenied,
    /// The desktop refused the lease.
    LeaseRefused,
    /// The router's breaker paused the session.
    PolicyDenials,
    /// The backend cannot run this mode.
    ModeUnsupported(RunMode),
    /// The backend or the model lacks a capability the task needs.
    NeedsCapability,
    /// The screen showed text that looked like instructions for the assistant, and the person
    /// said to stop. The kind is the first thing the check found.
    ScreenSuspicious(ScreenKind),
}

/// Who cancelled a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CancelledBy {
    /// The person.
    User,
    /// The kill chord or `StopAll`.
    KillSwitch,
    /// The planner.
    Planner,
}

/// How a run ended. The run's report to its requester is a `prov::Message` whose status is
/// [`CuaOutcome::status`]; this is the detail the event log and the run row keep.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CuaOutcome {
    /// Finished what was asked.
    Done,
    /// Could not.
    Failed(FailReason),
    /// Could not start or go on.
    Blocked(BlockedOn),
    /// Stopped.
    Cancelled(CancelledBy),
    /// Ran out of this.
    BudgetOut(RunBudgetKind),
}

impl CuaOutcome {
    /// The status of the report a run with this outcome sends: `Done` is done, a cancel is
    /// cancelled, everything else is failed.
    pub fn status(&self) -> ReportStatus {
        match self {
            CuaOutcome::Done => ReportStatus::Done,
            CuaOutcome::Cancelled(_) => ReportStatus::Cancelled,
            CuaOutcome::Failed(_) | CuaOutcome::Blocked(_) | CuaOutcome::BudgetOut(_) => {
                ReportStatus::Failed
            }
        }
    }
}

/// Why a mode is unsupported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModeGap {
    /// The compositor stops rendering hidden windows, so nothing can be captured.
    HiddenCaptureWithheld,
    /// Hidden windows render, but the backend delivers no input to a window on a workspace that
    /// is not shown (a stock compositor: the seat is shared).
    NoHiddenInput,
    /// There is no nested compositor to run the window in.
    NoNestedHost,
    /// The window is the shell's own.
    ShellWindow,
}

/// What a backend can do for a run mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ModeSupport {
    /// Everything, with the person's cursor and focus untouched.
    Full,
    /// On a shared seat: the run works while the person keeps their hands off; physical input
    /// pauses it.
    HandsOff,
    /// Not at all.
    Unsupported(ModeGap),
}

impl ModeSupport {
    /// The `Modes` property's value for a mode: `full`, `hands_off` or `unsupported:<gap>`.
    pub fn wire(&self) -> String {
        match self {
            ModeSupport::Full => "full".to_owned(),
            ModeSupport::HandsOff => "hands_off".to_owned(),
            ModeSupport::Unsupported(gap) => {
                format!("unsupported:{}", crate::slug_of(gap).unwrap_or_default())
            }
        }
    }

    /// The support a property value names, if it names one.
    pub fn from_wire(text: &str) -> Option<ModeSupport> {
        match text {
            "full" => Some(ModeSupport::Full),
            "hands_off" => Some(ModeSupport::HandsOff),
            other => {
                let gap = other.strip_prefix("unsupported:")?;
                serde_json::from_value(serde_json::Value::String(gap.to_owned()))
                    .ok()
                    .map(ModeSupport::Unsupported)
            }
        }
    }
}

/// The `Modes` property: mode slug to support string, for the modes given.
pub fn modes_wire(support: &[(RunMode, ModeSupport)]) -> BTreeMap<String, String> {
    support
        .iter()
        .filter_map(|(mode, how)| Some((crate::slug_of(mode)?, how.wire())))
        .collect()
}

/// The `Backend` property: the backend's slug, or `none` when cuad has no desktop.
pub fn backend_wire(kind: Option<BackendKind>) -> String {
    kind.and_then(|k| crate::slug_of(&k))
        .unwrap_or_else(|| "none".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_state_slugs_are_stable() {
        let table = [
            (RunStateSlug::Granting, "granting"),
            (RunStateSlug::Leasing, "leasing"),
            (RunStateSlug::Observing, "observing"),
            (RunStateSlug::Thinking, "thinking"),
            (RunStateSlug::Authorising, "authorising"),
            (RunStateSlug::Confirming, "confirming"),
            (RunStateSlug::Acting, "acting"),
            (RunStateSlug::Settling, "settling"),
            (RunStateSlug::Verifying, "verifying"),
            (RunStateSlug::WaitingForUser, "waiting_for_user"),
            (RunStateSlug::Paused, "paused"),
            (RunStateSlug::TakenOver, "taken_over"),
            (RunStateSlug::Finished, "finished"),
        ];
        for (state, slug) in table {
            assert_eq!(crate::slug_of(&state).as_deref(), Some(slug));
            assert_eq!(
                serde_json::from_str::<RunStateSlug>(&format!("\"{slug}\"")).unwrap(),
                state
            );
        }
    }

    #[test]
    fn every_outcome_maps_to_one_report_status() {
        let table = [
            (CuaOutcome::Done, ReportStatus::Done),
            (
                CuaOutcome::Failed(FailReason::WindowGone),
                ReportStatus::Failed,
            ),
            (
                CuaOutcome::Failed(FailReason::Infeasible),
                ReportStatus::Failed,
            ),
            (
                CuaOutcome::Blocked(BlockedOn::ConsentDenied),
                ReportStatus::Failed,
            ),
            (
                CuaOutcome::Blocked(BlockedOn::ModeUnsupported(RunMode::AgentWorkspace)),
                ReportStatus::Failed,
            ),
            (
                CuaOutcome::Cancelled(CancelledBy::User),
                ReportStatus::Cancelled,
            ),
            (
                CuaOutcome::Cancelled(CancelledBy::KillSwitch),
                ReportStatus::Cancelled,
            ),
            (
                CuaOutcome::BudgetOut(RunBudgetKind::Spend),
                ReportStatus::Failed,
            ),
        ];
        for (outcome, status) in table {
            assert_eq!(outcome.status(), status, "{outcome:?}");
        }
    }

    #[test]
    fn outcomes_pin_their_json() {
        let table = [
            (CuaOutcome::Done, r#"{"kind":"done"}"#),
            (
                CuaOutcome::Failed(FailReason::CheckFailed),
                r#"{"kind":"failed","v":"check_failed"}"#,
            ),
            (
                CuaOutcome::Blocked(BlockedOn::ModeUnsupported(RunMode::NestedSession)),
                r#"{"kind":"blocked","v":{"kind":"mode_unsupported","v":"nested_session"}}"#,
            ),
            (
                CuaOutcome::Blocked(BlockedOn::ScreenSuspicious(ScreenKind::Instruction)),
                r#"{"kind":"blocked","v":{"kind":"screen_suspicious","v":"instruction"}}"#,
            ),
            (
                CuaOutcome::Cancelled(CancelledBy::KillSwitch),
                r#"{"kind":"cancelled","v":"kill_switch"}"#,
            ),
            (
                CuaOutcome::BudgetOut(RunBudgetKind::Stall),
                r#"{"kind":"budget_out","v":"stall"}"#,
            ),
        ];
        for (outcome, json) in table {
            assert_eq!(serde_json::to_string(&outcome).unwrap(), json);
            assert_eq!(serde_json::from_str::<CuaOutcome>(json).unwrap(), outcome);
        }
    }

    #[test]
    fn mode_support_wire_strings_round_trip() {
        let table = [
            (ModeSupport::Full, "full"),
            (ModeSupport::HandsOff, "hands_off"),
            (
                ModeSupport::Unsupported(ModeGap::NoHiddenInput),
                "unsupported:no_hidden_input",
            ),
            (
                ModeSupport::Unsupported(ModeGap::HiddenCaptureWithheld),
                "unsupported:hidden_capture_withheld",
            ),
            (
                ModeSupport::Unsupported(ModeGap::NoNestedHost),
                "unsupported:no_nested_host",
            ),
            (
                ModeSupport::Unsupported(ModeGap::ShellWindow),
                "unsupported:shell_window",
            ),
        ];
        for (support, text) in table {
            assert_eq!(support.wire(), text);
            assert_eq!(ModeSupport::from_wire(text), Some(support));
        }
        assert_eq!(ModeSupport::from_wire("unsupported:nonsense"), None);
        assert_eq!(ModeSupport::from_wire("mostly"), None);
    }

    #[test]
    fn the_modes_property_and_the_backend_property_use_slugs() {
        let modes = modes_wire(&[
            (RunMode::InPlace, ModeSupport::HandsOff),
            (
                RunMode::AgentWorkspace,
                ModeSupport::Unsupported(ModeGap::NoHiddenInput),
            ),
        ]);
        assert_eq!(modes.get("in_place").map(String::as_str), Some("hands_off"));
        assert_eq!(
            modes.get("agent_workspace").map(String::as_str),
            Some("unsupported:no_hidden_input")
        );
        assert_eq!(backend_wire(Some(BackendKind::KwinStock)), "kwin_stock");
        assert_eq!(backend_wire(Some(BackendKind::Casement)), "casement");
        assert_eq!(backend_wire(None), "none");
    }
}
