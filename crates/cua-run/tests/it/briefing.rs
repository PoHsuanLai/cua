//! The task line and hints the model's session opens with.

use crate::support::{run_id, task, user_goal};
use cua_bus::{CuaRecord, SuccessCheck};
use cua_run::{briefing_hints, task_line};
use prov::Labelled;

fn asked(text: &str) -> Labelled<String> {
    Labelled::new(text.to_owned(), user_goal().label)
}

#[test]
fn the_task_line_is_the_persons_sentence_when_the_router_gave_it() {
    let mut t = task(SuccessCheck::ModelSays);
    assert_eq!(task_line(&t), "download the March invoice");
    t.asked = Some(asked("get me the invoice"));
    assert_eq!(task_line(&t), "get me the invoice");
}

#[test]
fn the_plan_comes_first_then_the_shared_hints() {
    let mut t = task(SuccessCheck::ModelSays);
    t.asked = Some(asked("get me the invoice"));
    let shared = vec!["The desktop is KDE.".to_owned()];
    assert_eq!(
        briefing_hints(&t, &run_id(), &shared),
        [
            "The assistant's plan for this run: download the March invoice",
            "The desktop is KDE."
        ]
    );
}

#[test]
fn the_same_words_add_no_plan_hint() {
    let mut t = task(SuccessCheck::ModelSays);
    t.asked = Some(asked("download the March invoice"));
    assert!(briefing_hints(&t, &run_id(), &[]).is_empty());
}

#[test]
fn no_earlier_runs_add_no_heading() {
    let mut t = task(SuccessCheck::ModelSays);
    t.earlier = Some(Labelled::new(Vec::<CuaRecord>::new(), user_goal().label));
    assert!(briefing_hints(&t, &run_id(), &[]).is_empty());
}

#[test]
fn earlier_attempts_follow_the_plan_under_a_heading_of_their_own() {
    let mut t = task(SuccessCheck::ModelSays);
    t.earlier = Some(Labelled::new(
        vec![crate::attempts::started(2, t.app.as_str())],
        user_goal().label,
    ));
    let hints = briefing_hints(&t, &run_id(), &["shared".to_owned()]);
    assert_eq!(
        hints[0],
        "Earlier attempts at this (check the screen before trusting them):"
    );
    assert_eq!(hints[1], "Attempt 1:");
    assert_eq!(hints.last().map(String::as_str), Some("shared"));
}
