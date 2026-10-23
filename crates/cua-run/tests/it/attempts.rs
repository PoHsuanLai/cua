//! Earlier attempts in the model's words: grouped by run, filtered by app, the newest few, the
//! current run left out, repeats collapsed, and never a coordinate or typed text.

use cua_action::RunMode;
use cua_action::{
    Button, Chord, ClickCount, Coord, CuaAction, DeviceSize, Point, Repeat, Target, TypedText,
    WindowSpace,
};
use cua_bus::{
    ActionResult, AuthorisedBy, BlockedOn, CuaOutcome, CuaRecord, FrameNote, LocalRefusal, Refusal,
    RunBudget, RunBudgetKind, TargetedAction, ThoughtDigest, WindowRef,
};
use cua_desktop::{BackendKind, Digest32};
use cua_run::{Limits, Mark, Tried, Verb, digest, digest_within, stall_words, steps_words};
use docket_core::EffectBasis;
use porter_core::{AppName, Count, MicroUsd};
use porter_infer::StepIndex;
use prov::{Effect, RunId, SessionId};
use std::collections::BTreeSet;

use crate::support::{task, user_goal};

fn app(id: &str) -> AppName {
    AppName::parse(id).unwrap()
}

fn run(n: u32) -> RunId {
    RunId::parse(&format!("r-{n}")).unwrap()
}

pub fn started(n: u32, of: &str) -> CuaRecord {
    CuaRecord::RunStarted {
        run: run(n),
        session: SessionId::parse("s-2").unwrap(),
        conversation: SessionId::parse("s-1").unwrap(),
        goal: user_goal(),
        app: app(of),
        space: task(cua_bus::SuccessCheck::ModelSays).space,
        mode: RunMode::InPlace,
        backend: BackendKind::KwinStock,
        budget: RunBudget::PROPOSED,
    }
}

fn step(n: u32, action: CuaAction<WindowSpace>, result: ActionResult) -> CuaRecord {
    CuaRecord::Step {
        run: run(n),
        n: StepIndex(1),
        thought: ThoughtDigest(Digest32([7; 32])),
        action: TargetedAction {
            window: WindowRef(1),
            action,
        },
        effect: Effect::UndoableWrite,
        basis: EffectBasis::WindowClass(cua_action::WindowClass::Ordinary),
        authorised_by: AuthorisedBy::Policy,
        result,
        frame: FrameNote::NotKept {
            digest: maskcap::FrameDigest(Digest32([9; 32])),
            size: DeviceSize { w: 852, h: 532 },
        },
        input: None,
    }
}

fn finished(n: u32, outcome: CuaOutcome) -> CuaRecord {
    CuaRecord::Finished {
        run: run(n),
        outcome,
        steps: Count(3),
        cost: MicroUsd(0),
        report: None,
    }
}

fn click_at(x: u32, y: u32) -> CuaAction<WindowSpace> {
    CuaAction::Click {
        at: Target::Point(Point::new(Coord(x), Coord(y))),
        button: Button::Left,
        count: ClickCount::One,
        mods: BTreeSet::new(),
    }
}

fn drag() -> CuaAction<WindowSpace> {
    CuaAction::Drag {
        from: Target::Point(Point::new(Coord(1234), Coord(5678))),
        to: Target::Point(Point::new(Coord(4321), Coord(8765))),
        button: Button::Left,
    }
}

fn typing() -> CuaAction<WindowSpace> {
    CuaAction::Type {
        text: TypedText::new("90210").unwrap(),
    }
}

fn enter() -> CuaAction<WindowSpace> {
    CuaAction::Key {
        chord: Chord::new(BTreeSet::new(), keyboard_types::Key::Enter).unwrap(),
        repeat: Repeat::ONCE,
    }
}

fn no_progress() -> ActionResult {
    ActionResult::Refused(Refusal::Local(LocalRefusal::NoProgress))
}

/// Three steps and an end, for run `n` in `of`.
fn attempt(n: u32, of: &str) -> Vec<CuaRecord> {
    vec![
        started(n, of),
        step(n, click_at(4817, 3), ActionResult::Done),
        step(n, click_at(4817, 3), no_progress()),
        step(n, drag(), ActionResult::Done),
        finished(n, CuaOutcome::BudgetOut(RunBudgetKind::Stall)),
    ]
}

#[test]
fn a_run_is_one_attempt_with_its_steps_and_its_end() {
    let lines = digest(&attempt(1, "org.gimp.GIMP"), &app("org.gimp.GIMP"), None);
    assert_eq!(
        lines,
        vec![
            "Attempt 1:",
            "Tried: click, click (no visible change), drag.",
            "The screen stopped changing.",
        ]
    );
}

#[test]
fn runs_in_other_apps_are_left_out() {
    let mut records = attempt(1, "org.gimp.GIMP");
    records.extend(attempt(2, "org.mozilla.firefox"));
    let lines = digest(&records, &app("org.mozilla.firefox"), None);
    assert_eq!(lines.iter().filter(|l| l.starts_with("Attempt")).count(), 1);
}

#[test]
fn the_last_three_runs_stay_numbered_from_the_first_kept() {
    let records: Vec<CuaRecord> = (1..=5).flat_map(|n| attempt(n, "org.gimp.GIMP")).collect();
    let lines = digest(&records, &app("org.gimp.GIMP"), None);
    let heads: Vec<&String> = lines.iter().filter(|l| l.starts_with("Attempt")).collect();
    assert_eq!(heads, ["Attempt 3:", "Attempt 4:", "Attempt 5:"]);
}

#[test]
fn the_current_run_is_not_an_earlier_attempt() {
    let mut records = attempt(1, "org.gimp.GIMP");
    records.extend(attempt(2, "org.gimp.GIMP"));
    let lines = digest(&records, &app("org.gimp.GIMP"), Some(&run(2)));
    assert_eq!(lines.iter().filter(|l| l.starts_with("Attempt")).count(), 1);
}

#[test]
fn repeats_collapse_and_marks_follow_the_verb() {
    let tried = [
        Tried {
            verb: Verb::Click,
            mark: Mark::Done,
        },
        Tried {
            verb: Verb::Click,
            mark: Mark::Done,
        },
        Tried {
            verb: Verb::Drag,
            mark: Mark::NoChange,
        },
        Tried {
            verb: Verb::Drag,
            mark: Mark::NoChange,
        },
        Tried {
            verb: Verb::Drag,
            mark: Mark::NoChange,
        },
        Tried {
            verb: Verb::KeyPress,
            mark: Mark::Refused,
        },
        Tried {
            verb: Verb::Typing,
            mark: Mark::Failed,
        },
        Tried {
            verb: Verb::Scroll,
            mark: Mark::NotRun,
        },
    ];
    assert_eq!(
        steps_words(&tried),
        "click ×2, drag ×3 (no visible change), key press (refused), typing (failed), \
         scroll (not run)"
    );
}

#[test]
fn a_long_run_of_steps_keeps_the_newest_groups() {
    let tried: Vec<Tried> = (0..20)
        .map(|i| Tried {
            verb: if i % 2 == 0 {
                Verb::Click
            } else {
                Verb::Scroll
            },
            mark: Mark::Done,
        })
        .collect();
    let words = steps_words(&tried);
    assert!(words.starts_with("… "), "{words}");
    assert_eq!(words.matches(", ").count(), 7, "{words}");
}

#[test]
fn whole_attempts_go_from_the_oldest_end_to_fit_the_lines() {
    let records: Vec<CuaRecord> = (1..=3).flat_map(|n| attempt(n, "org.gimp.GIMP")).collect();
    let limits = Limits {
        attempts: 3,
        lines: 7,
    };
    let lines = digest_within(&records, &app("org.gimp.GIMP"), None, limits);
    assert_eq!(lines.len(), 6);
    assert_eq!(lines[0], "Attempt 2:");
    assert!(digest(&records, &app("org.gimp.GIMP"), None).len() <= Limits::BRIEFING.lines);
}

#[test]
fn how_a_run_ended_is_said_in_plain_words() {
    let mut records = vec![started(1, "org.gimp.GIMP")];
    records.push(finished(1, CuaOutcome::Done));
    records.push(started(2, "org.gimp.GIMP"));
    records.push(finished(2, CuaOutcome::Blocked(BlockedOn::ConsentDenied)));
    let lines = digest(&records, &app("org.gimp.GIMP"), None);
    assert_eq!(lines[1], "Tried: no actions.");
    assert_eq!(lines[2], "It said it was done.");
    assert_eq!(lines[5], "Computer use was not allowed for this app.");
}

#[test]
fn no_coordinate_and_no_typed_text_is_ever_said() {
    let mut records = attempt(1, "org.gimp.GIMP");
    records.insert(1, step(1, typing(), ActionResult::Done));
    records.insert(2, step(1, enter(), ActionResult::Done));
    let text = digest(&records, &app("org.gimp.GIMP"), None).join("\n");
    for secret in ["4817", "1234", "5678", "4321", "8765", "90210"] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    assert!(text.contains("typing") && text.contains("key press"));
}

#[test]
fn a_stall_says_what_was_tried() {
    let click = Tried {
        verb: Verb::Click,
        mark: Mark::Done,
    };
    let drag = Tried {
        verb: Verb::Drag,
        mark: Mark::NoChange,
    };
    assert_eq!(
        stall_words(&[click, click, drag, drag, drag]),
        "The screen stopped changing after 5 steps: click ×2, drag ×3 (no visible change)."
    );
    assert_eq!(stall_words(&[]), "The screen stopped changing.");
}

#[test]
fn an_input_that_ran_and_changed_nothing_is_marked() {
    let seen = Tried::of(
        &click_at(1, 1),
        &ActionResult::Done,
        cua_run::Effect::NoVisibleChange,
    );
    assert_eq!(
        seen,
        Some(Tried {
            verb: Verb::Click,
            mark: Mark::NoChange
        })
    );
    let waited = Tried::of(
        &crate::support::wait(500),
        &ActionResult::Done,
        cua_run::Effect::NoVisibleChange,
    );
    assert_eq!(
        waited,
        Some(Tried {
            verb: Verb::Wait,
            mark: Mark::Done
        })
    );
}

fn seen(n: u32, effect: cua_run::Effect) -> CuaRecord {
    CuaRecord::Seen {
        run: run(n),
        n: StepIndex(2),
        effect,
    }
}

#[test]
fn a_stored_look_marks_the_inputs_since_the_one_before() {
    let records = vec![
        started(1, "org.gimp.GIMP"),
        step(1, click_at(1, 1), ActionResult::Done),
        seen(1, cua_run::Effect::Changed),
        step(1, click_at(2, 2), ActionResult::Done),
        step(1, drag(), ActionResult::Done),
        seen(1, cua_run::Effect::NoVisibleChange),
        step(1, crate::support::wait(500), ActionResult::Done),
        seen(1, cua_run::Effect::NoVisibleChange),
        seen(2, cua_run::Effect::NoVisibleChange),
    ];
    let lines = digest(&records, &app("org.gimp.GIMP"), None);
    assert_eq!(
        lines,
        vec![
            "Attempt 1:",
            "Tried: click, click (no visible change), drag (no visible change), wait.",
        ]
    );
}
