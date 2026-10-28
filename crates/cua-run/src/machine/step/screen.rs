//! The screen check in the phase table: what the machine does with a look before the model is
//! called, and with the person's answer.
//!
//! `Observed` -> check the screen -> clear: call the model. Suspicious: record it, and unless the
//! person already said to go on for this window and this kind, ask ("Continue?"). A yes calls the
//! model and remembers the answer; a no ends the run as `Blocked(ScreenSuspicious(kind))`. A run
//! that has seen a suspicious screen asks again before a step that sends something out or removes
//! something, whatever the router said.

use super::authorise::raise;
use crate::machine::effect::RunEffect;
use crate::machine::finish::finish_plain;
use crate::machine::ops::{enter, head_effect};
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::machine::track::Held;
use crate::screen::{
    Asking, PatternCheck, ReaderMode, ReaderWait, Reason, ScreenRead, ScreenText, ScreenVerdict,
    ScreenView, WindowKey,
};
use crate::sensitivity::{FORCED_NO, FORCED_YES};
use crate::turn::turn_after;
use cua_bus::{BlockedOn, ConfirmBy, CuaOutcome, CuaRecord, FailReason, ScreenAsk, ScreenKind};
use cua_desktop::GlowState;
use porter_infer::StepIndex;
use prov::{Effect, Labelled};

/// The question when the screen holds text that reads as instructions for the assistant.
const QUESTION: &str =
    "The screen shows text that looks like instructions for the assistant. Continue?";

/// The question when the only finding is hidden characters.
const HIDDEN_QUESTION: &str = "The screen shows text with characters hidden in it. Continue?";

/// Looks at the screen the model is about to be shown.
pub(super) fn stage(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex) {
    let (verdict, text) = match run.working.as_deref() {
        Some(obs) => {
            let view = ScreenView::of(obs);
            (
                PatternCheck::new(&run.env.rules.injection).check(&view),
                ScreenText::of(&view),
            )
        }
        None => (ScreenVerdict::Clear, None),
    };
    match verdict {
        ScreenVerdict::Clear => clear(run, fx, n, text),
        suspicious => flagged(run, fx, n, &suspicious),
    }
}

/// The model's turn from the step's observation.
pub(super) fn call_model(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex) {
    let Some(obs) = run.working.as_deref() else {
        finish_plain(run, fx, CuaOutcome::Failed(FailReason::Capture));
        return;
    };
    let (request, frame) = turn_after(obs, run.track.effect);
    enter(run, fx, RunPhase::Thinking(n));
    fx.push(RunEffect::CallModel {
        request: Box::new(request),
        frame,
    });
}

/// The deterministic check found nothing: the reader is asked when it is on, else the model.
fn clear(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, text: Option<ScreenText>) {
    match (run.env.reader, text) {
        (ReaderMode::On, Some(text)) => {
            run.track.screen.reader = ReaderWait::Reading;
            fx.push(RunEffect::ReadScreen(text));
        }
        _ => call_model(run, fx, n),
    }
}

/// The reader's answer about the look the run is waiting on.
pub(super) fn screen_read(run: &mut Run, fx: &mut Vec<RunEffect>, read: ScreenRead) {
    let RunPhase::Observing(n) = run.phase else {
        return;
    };
    if run.track.screen.reader != ReaderWait::Reading {
        return;
    }
    run.track.screen.reader = ReaderWait::Idle;
    match read {
        ScreenRead::Instructions => flagged(run, fx, n, &ScreenVerdict::of(vec![Reason::Reader])),
        ScreenRead::Clear | ScreenRead::Unavailable => call_model(run, fx, n),
    }
}

/// The words after the fixed question: what else is wrong with the text, and a short excerpt of
/// it, marked as found on the screen.
fn details_of(verdict: &ScreenVerdict) -> Vec<String> {
    let kinds = verdict.kinds();
    let mut details = Vec::new();
    let hidden_only = hidden_only(&kinds);
    if kinds.contains(&ScreenKind::HiddenCharacters) && !hidden_only {
        details.push("Some of it is hidden from view.".to_owned());
    }
    if kinds.contains(&ScreenKind::UnseenText) {
        details.push("Some of it is not visible on the screen.".to_owned());
    }
    if let Some(excerpt) = verdict.reasons().iter().find_map(Reason::excerpt) {
        details.push(format!(
            "Text found on the screen: \"{}\"",
            excerpt.as_str().replace('"', "'")
        ));
    }
    details
}

/// Whether hidden characters are all that was found (an unseen node counts with what it holds).
fn hidden_only(kinds: &[ScreenKind]) -> bool {
    kinds
        .iter()
        .all(|k| matches!(k, ScreenKind::HiddenCharacters | ScreenKind::UnseenText))
        && kinds.contains(&ScreenKind::HiddenCharacters)
}

/// The first line says what was found: instructions, or only hidden characters.
fn question_of(verdict: &ScreenVerdict) -> String {
    let first = if hidden_only(&verdict.kinds()) {
        HIDDEN_QUESTION
    } else {
        QUESTION
    };
    let mut lines = vec![first.to_owned()];
    lines.extend(details_of(verdict));
    lines.join("\n")
}

/// The screen is suspicious: it is recorded, and the person is asked unless they already said to
/// go on for this window and this kind in this run.
fn flagged(run: &mut Run, fx: &mut Vec<RunEffect>, n: StepIndex, verdict: &ScreenVerdict) {
    let Some(obs) = run.working.as_deref() else {
        call_model(run, fx, n);
        return;
    };
    let window = WindowKey::of(&obs.window.app, &obs.window.title.value);
    let kinds = verdict.kinds();
    let remembered = kinds
        .iter()
        .all(|k| run.track.screen.allowed.contains(&(window.clone(), *k)));
    run.track.screen.suspicious = true;
    let ask = if remembered {
        ScreenAsk::Remembered
    } else {
        ScreenAsk::Asked
    };
    fx.push(RunEffect::Record(Box::new(CuaRecord::ScreenSuspicious {
        run: run.id.clone(),
        n,
        kinds: kinds.clone(),
        ask,
    })));
    if remembered {
        call_model(run, fx, n);
        return;
    }
    run.track.screen.asking = Some(Asking { window, kinds });
    run.track.held = Held::SuspiciousScreen;
    raise(run, fx, n, &question_of(verdict), &[FORCED_YES, FORCED_NO]);
}

/// The person's answer to the question about a suspicious screen: yes remembers the window and
/// the kinds and calls the model; anything else ends the run.
pub(super) fn answered(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    n: StepIndex,
    answer: &Labelled<String>,
) {
    run.track.held = Held::Nothing;
    let asking = run.track.screen.asking.take();
    if answer.value == FORCED_YES {
        fx.push(RunEffect::Glow(GlowState::Working));
        fx.push(RunEffect::Record(Box::new(CuaRecord::Confirmed {
            run: run.id.clone(),
            n,
            by: ConfirmBy::ShellPrompt,
        })));
        if let Some(asked) = asking {
            for kind in asked.kinds {
                run.track.screen.allowed.push((asked.window.clone(), kind));
            }
        }
        call_model(run, fx, n);
    } else {
        let kind = asking
            .and_then(|asked| asked.kinds.first().copied())
            .unwrap_or(ScreenKind::Instruction);
        finish_plain(
            run,
            fx,
            CuaOutcome::Blocked(BlockedOn::ScreenSuspicious(kind)),
        );
    }
}

/// The effect of the head action, when the run has seen a suspicious screen and the action sends
/// something out or removes something.
pub(super) fn suspicious_head(run: &Run) -> Option<Effect> {
    if !run.track.screen.suspicious {
        return None;
    }
    head_effect(run).filter(|effect| *effect >= Effect::Outbound)
}

/// What the step does, in the words the question uses.
fn doing(effect: Effect) -> &'static str {
    match effect {
        Effect::Destructive => "removes something for good",
        Effect::Outbound => "sends something to someone else",
        _ => "does something outside this window",
    }
}

/// The router said run, but the screen was suspicious earlier and this step goes out or removes:
/// the person is asked first, and the head action stays queued until they answer.
pub(super) fn hold_after_suspicion(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    n: StepIndex,
    effect: Effect,
) {
    run.track.held = Held::AfterSuspicion;
    let question = format!(
        "The screen earlier showed text that looked like instructions for the assistant, and the next step {}. Go ahead with it?",
        doing(effect)
    );
    raise(run, fx, n, &question, &[FORCED_YES, FORCED_NO]);
}
