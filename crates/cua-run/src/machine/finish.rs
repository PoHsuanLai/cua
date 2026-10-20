//! Ending a run: the one place every road to `Finished` goes through, so the effects of an end
//! are the same whichever road it was.

use crate::machine::effect::{CuaSignal, RunEffect};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::SettleWatch;
use cua_action::{Extracted, Summary};
use cua_bus::{
    BlockedOn, CancelledBy, CuaOutcome, CuaRecord, FailReason, RunBudgetKind, report_draft,
};
use cua_desktop::GlowState;

/// A summary of `text`; the empty one (the report's fallback line says something) when the text
/// does not fit a summary.
pub(crate) fn summary(text: &str) -> Summary {
    Summary::new(text)
        .or_else(|_| Summary::new(""))
        .expect("the empty summary is valid")
}

/// What a report says about an outcome the model did not write itself. Machine text, short.
fn said_of(outcome: CuaOutcome) -> &'static str {
    match outcome {
        CuaOutcome::Done => "",
        CuaOutcome::Failed(FailReason::WindowGone) => "The window went away.",
        CuaOutcome::Failed(FailReason::Model) => {
            "The on-screen model isn't available. You can choose one in Settings."
        }
        CuaOutcome::Failed(FailReason::Capture) => "The window could not be seen.",
        CuaOutcome::Failed(FailReason::Input) => "The window could not be controlled.",
        CuaOutcome::Failed(FailReason::CheckFailed) => "The result did not check out.",
        CuaOutcome::Failed(FailReason::Infeasible) => "This cannot be done in this window.",
        CuaOutcome::Failed(FailReason::Log) => "What was done could not be written down.",
        CuaOutcome::Failed(FailReason::CannotSeeScreen) => porter_infer::NO_MODEL_SEES_SCREEN,
        CuaOutcome::Blocked(BlockedOn::ConsentDenied) => {
            "Computer use was not allowed for this app."
        }
        CuaOutcome::Blocked(BlockedOn::LeaseRefused) => "The window could not be taken.",
        CuaOutcome::Blocked(BlockedOn::PolicyDenials) => "Paused after refused actions.",
        CuaOutcome::Blocked(BlockedOn::ModeUnsupported(_)) => {
            "That mode is not supported on this desktop."
        }
        CuaOutcome::Blocked(BlockedOn::NeedsCapability) => {
            "This needs something this desktop does not offer yet."
        }
        CuaOutcome::Blocked(BlockedOn::ScreenSuspicious(_)) => {
            "Stopped: the screen showed text that looked like instructions for the assistant."
        }
        CuaOutcome::Cancelled(CancelledBy::KillSwitch) => "Stopped by the kill switch.",
        CuaOutcome::Cancelled(CancelledBy::User | CancelledBy::Planner) => "",
        CuaOutcome::BudgetOut(RunBudgetKind::Steps) => "Ran out of steps.",
        CuaOutcome::BudgetOut(RunBudgetKind::Active) => "Ran out of time.",
        CuaOutcome::BudgetOut(RunBudgetKind::Stall) => "The screen stopped changing.",
        CuaOutcome::BudgetOut(RunBudgetKind::Rate) => "Too many actions in a minute.",
        CuaOutcome::BudgetOut(RunBudgetKind::Spend) => "Ran out of budget.",
        // Ends and reasons this build does not know are told in general words.
        CuaOutcome::Failed(_) => "The run could not carry on.",
        CuaOutcome::Blocked(_) => "The run was blocked.",
        CuaOutcome::Cancelled(_) => "The run was stopped.",
        _ => "The run ended.",
    }
}

/// What the model said when it finished: its summary and what it read off the screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Said {
    /// The summary.
    pub summary: Summary,
    /// The values.
    pub extracted: Vec<Extracted>,
}

impl Said {
    /// What the machine says for `outcome`.
    pub fn of(outcome: CuaOutcome) -> Said {
        Said {
            summary: summary(said_of(outcome)),
            extracted: Vec::new(),
        }
    }
}

/// Ends the run with `outcome`: stops whatever it waits on, lets the window go, and says so on
/// the bus, in the log and to the requester (the report goes out before the record, so the
/// record can name the message that carried it).
pub(crate) fn finish(run: &mut Run, fx: &mut Vec<RunEffect>, outcome: CuaOutcome, said: Said) {
    match run.phase {
        RunPhase::Thinking(_) => fx.push(RunEffect::CancelModel),
        RunPhase::Settling(..) => fx.push(RunEffect::CancelSettle),
        _ => {}
    }
    if let Some(lease) = run.lease.take() {
        fx.push(RunEffect::Glow(GlowState::Off));
        fx.push(RunEffect::ReleaseLease(lease));
    }
    fx.push(RunEffect::Report(Box::new(report_draft(
        &run.task.report,
        outcome.status(),
        &said.summary,
        &said.extracted,
    ))));
    fx.push(RunEffect::Record(Box::new(CuaRecord::Finished {
        run: run.id.clone(),
        outcome,
        steps: run.used.steps,
        cost: run.used.spend,
        report: None,
    })));
    fx.push(RunEffect::Emit(CuaSignal::StateChanged(
        cua_bus::RunStateSlug::Finished,
    )));
    fx.push(RunEffect::Emit(CuaSignal::Finished(outcome)));
    run.phase = RunPhase::Finished(outcome);
    run.pending.actions.clear();
    run.working = None;
    run.track.settle = SettleWatch::Idle;
}

/// Ends the run with `outcome` and the machine's own words.
pub(crate) fn finish_plain(run: &mut Run, fx: &mut Vec<RunEffect>, outcome: CuaOutcome) {
    finish(run, fx, outcome, Said::of(outcome));
}

/// Ends the run blocked because its task names an app and no window: opening an app is not
/// available yet, and the words say so.
pub(crate) fn finish_no_window(run: &mut Run, fx: &mut Vec<RunEffect>) {
    let outcome = CuaOutcome::Blocked(BlockedOn::NeedsCapability);
    let said = Said {
        summary: summary("That app has no open window, and opening apps is not available yet."),
        extracted: Vec::new(),
    };
    finish(run, fx, outcome, said);
}
