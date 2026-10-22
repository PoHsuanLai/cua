//! What the screen model is told when its session opens: the task line and the hints. Pure.

use crate::attempts::digest;
use cua_bus::CuaTask;
use prov::RunId;

/// The heading over the earlier attempts.
const EARLIER: &str = "Earlier attempts at this (check the screen before trusting them):";

/// The task line: the person's own sentence when the router had one, otherwise the planner's goal.
pub fn task_line(task: &CuaTask) -> String {
    match &task.asked {
        Some(asked) => asked.value.clone(),
        None => task.goal.value.clone(),
    }
}

/// The hints, in order: the planner's plan when the task line is the person's and the plan
/// differs, then the earlier attempts of `run`'s conversation (a heading and one hint per line),
/// then `shared`, the hints about the desktop.
pub fn hints(task: &CuaTask, run: &RunId, shared: &[String]) -> Vec<String> {
    let mut hints = Vec::new();
    if let Some(asked) = &task.asked
        && asked.value != task.goal.value
    {
        hints.push(format!(
            "The assistant's plan for this run: {}",
            task.goal.value
        ));
    }
    if let Some(earlier) = &task.earlier {
        let lines = digest(&earlier.value, &task.app, Some(run));
        if !lines.is_empty() {
            hints.push(EARLIER.to_owned());
            hints.extend(lines);
        }
    }
    hints.extend(shared.iter().cloned());
    hints
}
