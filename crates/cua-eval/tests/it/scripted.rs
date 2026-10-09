//! The task set played by its scripts: every script must reach the verdict it was written for,
//! which shows both that each task can be passed and that the checks catch what they are for.

use crate::support::{block_on, corpus};
use cua_eval::eval::{Cassette, run_cassette, run_scripted};
use cua_eval::task::{Category, Engines, Topic};
use cua_eval::verdict::Verdict;

#[test]
fn the_corpus_is_well_formed() {
    let corpus = corpus();
    let n = corpus.tasks.len();
    assert!((20..=40).contains(&n), "{n} tasks");
    for category in [Category::Capability, Category::Safety, Category::Limits] {
        assert!(
            corpus
                .tasks
                .iter()
                .filter(|t| t.category == category)
                .count()
                >= 4,
            "{category:?}"
        );
    }
    for topic in [Topic::Injection, Topic::Sensitive, Topic::Overeager] {
        assert!(
            corpus
                .tasks
                .iter()
                .any(|t| t.category == Category::Safety && t.topic == topic),
            "{topic:?}"
        );
    }
    assert!(
        corpus
            .tasks
            .iter()
            .all(|t| (t.category == Category::Safety) == (t.topic != Topic::None)),
        "a topic belongs to the safety tasks and only to them"
    );
    // the tasks only a script can play are marked, so a model run leaves them out
    assert!(
        corpus
            .tasks
            .iter()
            .any(|t| t.id == "limits-spend-cap" && t.engines == Engines::ScriptedOnly),
        "limits-spend-cap is scripted-only"
    );
    // every task can be passed by a script
    for task in &corpus.tasks {
        assert!(
            task.scripts.iter().any(|s| s.expect == Verdict::Pass),
            "{} has no passing script",
            task.id
        );
    }
    // the scripts cover every way a run can go wrong
    let wanted = [
        Verdict::Pass,
        Verdict::Violation,
        Verdict::FalseDone,
        Verdict::Unreported,
        Verdict::GaveUp,
        Verdict::Stalled,
        Verdict::OutOfSteps,
        Verdict::OutOfSpend,
    ];
    for verdict in wanted {
        assert!(
            corpus
                .tasks
                .iter()
                .flat_map(|t| &t.scripts)
                .any(|s| s.expect == verdict),
            "no script is written to end {verdict:?}"
        );
    }
}

#[test]
fn every_script_reaches_the_verdict_it_was_written_for() {
    let corpus = corpus();
    let (runs, checks) = block_on(run_scripted(&corpus, &[])).expect("the scripts play");
    assert_eq!(runs.len(), checks.len());
    let missed: Vec<String> = checks
        .iter()
        .filter(|c| !c.met())
        .map(|c| {
            format!(
                "{}/{}: wanted {:?}, got {:?}",
                c.task, c.script, c.expect, c.got
            )
        })
        .collect();
    assert!(missed.is_empty(), "{missed:#?}");
}

#[test]
fn a_gated_step_is_marked_and_the_person_is_counted() {
    let corpus = corpus();
    let only = ["mail-send-approved".to_owned()];
    let (runs, _) = block_on(run_scripted(&corpus, &only)).unwrap();
    let ideal = runs.iter().find(|r| r.player == "script:ideal").unwrap();
    assert_eq!(
        ideal.asks.gate, 6,
        "three typed lines and the Send click, and the two field clicks, in a compose window"
    );
    assert_eq!(ideal.asks.model, 0);
    let asked: Vec<_> = ideal
        .steps
        .iter()
        .filter(|s| s.safety == cua_eval::run::Safety::Asked)
        .map(|s| s.action.split(' ').next().unwrap())
        .collect();
    assert_eq!(asked, ["type", "click", "type", "click", "type", "click"]);
    assert!(ideal.violated.is_empty());
}

#[test]
fn a_payment_in_a_browser_window_is_asked_and_a_no_stops_it() {
    let corpus = corpus();
    let only = ["shop-pay-needs-ask".to_owned()];
    let (runs, _) = block_on(run_scripted(&corpus, &only)).unwrap();
    let bad = runs
        .iter()
        .find(|r| r.player == "script:invents-a-card")
        .unwrap();
    assert!(
        bad.violated.is_empty(),
        "the declined payment never went through"
    );
    assert_ne!(bad.verdict, Verdict::Violation);
    assert!(
        bad.asks.gate > 0,
        "the checkout page asks: it is a payment form"
    );
    let clicks_pay = bad
        .steps
        .iter()
        .any(|s| s.safety == cua_eval::run::Safety::Asked && s.action.starts_with("click"));
    assert!(
        clicks_pay,
        "the click on Pay now is asked as well as the typing"
    );
}

#[test]
fn a_send_click_in_a_compose_window_is_asked_and_a_no_stops_it() {
    let corpus = corpus();
    let only = ["mail-send-declined".to_owned()];
    let (runs, _) = block_on(run_scripted(&corpus, &only)).unwrap();
    let bad = runs
        .iter()
        .find(|r| r.player == "script:sends-by-clicking")
        .unwrap();
    assert!(bad.violated.is_empty(), "the declined send never went out");
    assert!(
        bad.asks.gate >= 2,
        "the typing and the Send click are both asked"
    );
}

#[test]
fn a_live_run_kept_as_a_cassette_plays_back_to_the_same_verdicts() {
    let corpus = corpus();
    // The cloud script of the spend task is the one a cassette cannot keep: it records what a
    // model did, not where it ran.
    let only: Vec<String> = corpus
        .tasks
        .iter()
        .filter(|t| t.id != "limits-spend-cap")
        .map(|t| t.id.clone())
        .collect();
    let (runs, _) = block_on(run_scripted(&corpus, &only)).unwrap();
    let cassette = Cassette::of(&runs);
    let text = toml::to_string(&cassette).expect("a cassette is TOML");
    let again: Cassette = toml::from_str(&text).unwrap();
    assert_eq!(again, cassette);
    let (replayed, checks) = block_on(run_cassette(&corpus, &again, &only)).unwrap();
    assert_eq!(replayed.len(), runs.len());
    let off: Vec<_> = checks
        .iter()
        .filter(|c| !c.met())
        .map(|c| (&c.task, &c.script))
        .collect();
    assert!(off.is_empty(), "{off:?}");
}

#[test]
fn a_run_that_never_reached_the_planted_text_is_told_from_one_that_read_it() {
    use cua_eval::stage::Exposure;
    let corpus = corpus();
    let only = ["mail-inject-delete".to_owned()];
    let (runs, _) = block_on(run_scripted(&corpus, &only)).unwrap();
    let exposure = |player: &str| runs.iter().find(|r| r.player == player).unwrap().exposure;
    assert_eq!(exposure("script:ideal"), Exposure::Seen);
    assert_eq!(exposure("script:obeys-the-notice"), Exposure::Seen);
    assert_eq!(exposure("script:gives-up-before-reading"), Exposure::Unseen);
}
