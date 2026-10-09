//! The live path, with a provider that plays canned turns instead of an engine: the same prompt
//! building, frame preparation and reply reading a real model goes through.

use crate::support::{block_on, corpus};
use cua_eval::eval::run_live;
use cua_eval::live::{LiveError, Plan, Thinking};
use cua_eval::verdict::Verdict;
use model_provider::{
    ModelInfo, ModelName, Part, Script, ScriptedProvider, StopReason, Tokens, ToolCall, ToolCallId,
    ToolName, TurnEnd, TurnEvent, TurnUsage,
};
use std::sync::Arc;

const ENTRY: &str = include_str!("fixtures/holo-3.1-4b.toml");

fn call(arguments: &str) -> TurnEvent {
    TurnEvent::ToolCallDone(ToolCall {
        id: ToolCallId("c1".into()),
        name: ToolName::new("computer_use").unwrap(),
        input: model_provider::JsonText::new(arguments).unwrap(),
    })
}

fn turn(arguments: &str) -> Script {
    Script {
        events: vec![call(arguments)],
        end: Ok(TurnEnd {
            stop: StopReason::ToolUse,
            usage: TurnUsage::default(),
            served: ModelName("holo".into()),
            first_token: None,
        }),
    }
}

fn provider(scripts: Vec<Script>) -> Arc<ScriptedProvider> {
    Arc::new(ScriptedProvider::new(
        vec![ModelInfo {
            name: ModelName("holo".into()),
            loaded_context: Tokens(32768),
            trained_context: Tokens(32768),
        }],
        scripts,
    ))
}

#[test]
fn the_catalogue_entry_of_a_computer_use_model_makes_a_plan() {
    assert!(Plan::of(ENTRY, "holo", Thinking::Off).is_ok());
    assert!(Plan::of(ENTRY, "holo", Thinking::On).is_ok());
    assert!(matches!(
        Plan::of("nonsense", "holo", Thinking::Off),
        Err(LiveError::Entry(_))
    ));
}

#[test]
fn a_model_that_reads_the_screen_and_answers_passes_the_task_it_was_given() {
    // mail-read: click the message from Sam, then say what time lunch is. The row's middle is
    // (480, 158) in a 960x600 window, which is (500, 263) on the model's 1000-point grid.
    let provider = provider(vec![
        turn(r#"{"action":"click","x":500,"y":263}"#),
        turn(r#"{"action":"finish","status":"done","summary":"Lunch is at noon."}"#),
    ]);
    let plan = Plan::of(ENTRY, "holo", Thinking::Off).unwrap();
    let only = ["mail-read".to_owned()];
    let runs = block_on(run_live(
        &corpus(),
        provider.clone(),
        &plan,
        "test",
        1,
        &only,
    ))
    .unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].verdict, Verdict::Pass, "{:?}", runs[0]);
    assert_eq!(runs[0].steps.len(), 1, "the finish is not a step that acts");
    assert_eq!(runs[0].moves.len(), 2);
    let requests = provider.requests();
    assert_eq!(requests.len(), 2);
    let sent_image = requests[0]
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .any(|p| matches!(p, Part::Image(i) if i.bytes.0.starts_with(b"\x89PNG")));
    assert!(sent_image, "the screen goes to the model as a PNG");
}

#[test]
fn a_model_that_cannot_be_read_is_a_failed_run_and_not_a_crash() {
    let provider = provider(vec![]);
    let plan = Plan::of(ENTRY, "holo", Thinking::Off).unwrap();
    let only = ["mail-read".to_owned()];
    let runs = block_on(run_live(&corpus(), provider, &plan, "test", 1, &only)).unwrap();
    assert_ne!(runs[0].verdict, Verdict::Pass);
}

#[test]
fn only_the_tasks_a_model_can_play_are_played() {
    let provider = provider(vec![]);
    let plan = Plan::of(ENTRY, "holo", Thinking::Off).unwrap();
    let only = ["limits-spend-cap".to_owned()];
    let runs = block_on(run_live(&corpus(), provider, &plan, "test", 1, &only)).unwrap();
    assert!(runs.is_empty());
}
