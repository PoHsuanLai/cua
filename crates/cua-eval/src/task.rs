//! A task: where it starts, what the person asked, what counts as done, what must not happen,
//! and how much it may spend. Tasks are data files under `eval/tasks/`, over the surfaces under
//! `eval/surfaces/`.

use crate::gate::Person;
use crate::surface::{Act, Cond};
use crate::verdict::Verdict;
use serde::{Deserialize, Serialize};

/// The three kinds of task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    /// Can it do the job.
    Capability,
    /// Does it keep to what it was asked, and stop where it must.
    Safety,
    /// Does it stop when it should.
    Limits,
}

/// What kind of safety a task probes.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Topic {
    /// Not a safety task.
    #[default]
    None,
    /// The screen carries text that tells the agent to do something else.
    Injection,
    /// The window is one where the agent must stop and ask.
    Sensitive,
    /// The task is small and the screen invites more.
    Overeager,
}

/// Whether the right run asks the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskExpect {
    /// It never needs to; an ask is a nuisance.
    #[default]
    Never,
    /// It must ask at least once, through the gate or by its own question.
    Required,
}

/// How the right run ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndExpect {
    /// It says it is done, and it is.
    #[default]
    Done,
    /// It says it cannot.
    Infeasible,
    /// It may end any way but running out of something.
    Any,
}

/// Which models a task is played against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Engines {
    /// Both.
    #[default]
    Both,
    /// Only a script: the task is about a limit a local model cannot reach.
    ScriptedOnly,
}

/// Where a script's model says it ran.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Served {
    /// On this machine.
    #[default]
    Local,
    /// On a provider's servers, which the spend cap counts.
    Cloud,
}

/// One scripted run of a task, and what the eval must say about it. The scripts that behave
/// well show the task can be passed; the ones that misbehave show the checks catch it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScriptDef {
    /// Its name.
    pub name: String,
    /// The verdict the eval must reach.
    pub expect: Verdict,
    /// Where the model runs.
    #[serde(default)]
    pub served: Served,
    /// The moves, one per line (see `script`).
    pub steps: Vec<String>,
}

/// One task.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TaskDef {
    /// Its id.
    pub id: String,
    /// Its kind.
    pub category: Category,
    /// What it probes within safety.
    #[serde(default)]
    pub topic: Topic,
    /// The surface it runs on.
    pub surface: String,
    /// The screen it starts on.
    pub start: String,
    /// Done to the surface before the run.
    #[serde(default)]
    pub setup: Vec<Act>,
    /// What the person asked.
    pub goal: String,
    /// The most steps it may take.
    pub steps: u32,
    /// The most it may spend, in millionths of a dollar.
    #[serde(default)]
    pub spend_microusd: u64,
    /// How the person answers a confirmation.
    #[serde(default)]
    pub person: Person,
    /// Whether the right run asks.
    #[serde(default)]
    pub ask: AskExpect,
    /// How the right run ends.
    #[serde(default)]
    pub end: EndExpect,
    /// All of these hold at the end.
    pub success: Vec<Cond>,
    /// What the model's final words must contain, all of it, in any case.
    #[serde(default)]
    pub answer: Vec<String>,
    /// None of these may ever hold.
    #[serde(default)]
    pub forbid: Vec<Cond>,
    /// For an injection task: all of these hold while the injected text is on the screen, so a
    /// run that never got there is told from one that read it and did not obey.
    #[serde(default)]
    pub exposure: Vec<Cond>,
    /// Which models play it.
    #[serde(default)]
    pub engines: Engines,
    /// What the task is for, in a line.
    pub note: String,
    /// Its scripted runs.
    #[serde(default, rename = "script")]
    pub scripts: Vec<ScriptDef>,
}

/// A file of tasks.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct TaskFile {
    /// The tasks.
    #[serde(rename = "task")]
    pub tasks: Vec<TaskDef>,
}
