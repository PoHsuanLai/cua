//! The report: the groups, the safety rates and bounds, and the script checks.

use crate::support::{block_on, corpus};
use cua_eval::eval::run_scripted;
use cua_eval::report::{Report, render};

fn scripted_report() -> String {
    let corpus = corpus();
    let (runs, checks) = block_on(run_scripted(&corpus, &[])).expect("the scripts play");
    render(&Report {
        label: "test",
        engine: "scripts",
        tasks: &corpus.tasks,
        runs: &runs,
        checks: &checks,
    })
}

#[test]
fn the_report_has_every_section_and_every_task() {
    let text = scripted_report();
    for heading in [
        "# cua eval: test",
        "## Summary",
        "## Safety",
        "## Tasks",
        "## Script checks",
    ] {
        assert!(text.contains(heading), "{heading}");
    }
    for task in &corpus().tasks {
        assert!(text.contains(&format!("| {} |", task.id)), "{}", task.id);
    }
    for row in [
        "| capability |",
        "| safety |",
        "| limits |",
        "| injection |",
        "| sensitive window |",
        "| over-eager |",
    ] {
        assert!(text.contains(row), "{row}");
    }
    assert!(
        !text.contains("MISMATCH"),
        "every script met its expectation"
    );
}

#[test]
fn the_same_runs_make_the_same_report() {
    assert_eq!(scripted_report(), scripted_report());
}

#[test]
fn rates_come_with_bounds() {
    let text = scripted_report();
    let safety = text
        .lines()
        .find(|l| l.starts_with("| injection |"))
        .expect("an injection row");
    assert!(safety.matches('%').count() >= 3, "{safety}");
    assert!(
        text.contains("upper bound"),
        "the safety summary names its bound"
    );
}
