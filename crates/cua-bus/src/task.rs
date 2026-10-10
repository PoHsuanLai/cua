//! The task a run starts from: the `Start` body.

use crate::budget::RunBudget;
use crate::ids::ProcedureId;
use cua_desktop::{Stamp, ToplevelIdent};
use cua_types::NodeSelector;
use docket_core::{RunMode, Value};
use porter_core::AppName;
use prov::{Address, Labelled, MessageId, SessionId, SpaceId, ThreadId};
use serde::{Deserialize, Serialize};

/// Which window the run works in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TaskTarget {
    /// This window.
    Window(ToplevelIdent),
    /// A window of this app: cuad picks the window when exactly one qualifies; launching an app
    /// into the mode is a later wave, and a run started with this target ends `Blocked` until it
    /// lands.
    App(AppName),
}

/// Where the run came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RunOrigin {
    /// The planner asked for it.
    Planner,
    /// The person started it by hand and handed the window over, since this instant.
    Handover {
        /// When they started.
        since: Stamp,
    },
}

/// How the run knows it is done.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SuccessCheck {
    /// The model says so.
    ModelSays,
    /// The tree has this node.
    TreeHas(NodeSelector),
    /// The model says so and the tree has this node.
    Both(NodeSelector),
}

/// A learned procedure to use (a later wave; `None` is the only value built).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ProcedureUse {
    /// Plain run.
    None,
    /// Show the model the procedure as a hint.
    Hint(ProcedureId),
    /// Replay it.
    Replay(ProcedureId),
}

/// Where the run's reports go: the requester's address, and the conversation they continue.
/// Progress and the final report are `prov::Message`s of kind `Report`, sent through
/// `Intents1.Message.Send` (cua's role may send); the router stamps the id, the sender and the
/// label. A report carries no authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReportRoute {
    /// Whom the reports are for.
    pub to: Address,
    /// The thread they continue; none starts one.
    pub thread: Option<ThreadId>,
    /// The message that asked for the run.
    pub in_reply_to: Option<MessageId>,
}

/// The `Start` body: everything a run needs, built by cuad from the `cua.run.start` call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CuaTask {
    /// The app of the window the run works in (the per-app consent and the records name it).
    /// cuad resolves it before the task is built, whichever way the target was given.
    pub app: AppName,
    /// What to do: trusted when the person typed it, otherwise carrying the label of whoever
    /// wrote it.
    pub goal: Labelled<String>,
    /// Where.
    pub target: TaskTarget,
    /// How: the mode is concrete here (`auto` is resolved by cuad before the task is built).
    pub mode: RunMode,
    /// The Space the run acts in.
    pub space: SpaceId,
    /// The session the run acts for; the actor of its records is
    /// `Actor::Companion { session, role: Cua { run } }`.
    pub session: SessionId,
    /// The limits.
    pub budget: RunBudget,
    /// Values the task may use.
    pub data_in: Vec<Labelled<Value>>,
    /// How it knows it is done.
    pub success: SuccessCheck,
    /// Where it came from.
    pub origin: RunOrigin,
    /// A procedure to use.
    pub procedure: ProcedureUse,
    /// Where it reports.
    pub report: ReportRoute,
}
