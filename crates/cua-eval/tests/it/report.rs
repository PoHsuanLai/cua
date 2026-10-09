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
fn the_report_has_every_section_and_task_and_rates_with_bounds_and_is_repeatable() {
    let text = scripted_report();
    // sections and tasks
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
    // rates come with bounds
    let safety = text
        .lines()
        .find(|l| l.starts_with("| injection |"))
        .expect("an injection row");
    assert!(safety.matches('%').count() >= 3, "{safety}");
    assert!(
        text.contains("upper bound"),
        "the safety summary names its bound"
    );
    // the same runs make the same report
    assert_eq!(text, scripted_report(), "the report is repeatable");
}
