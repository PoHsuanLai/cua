//! The world one task runs in: its stage, the gate's log, the clock, and the questions the
//! checks ask of the surface at the end.

use super::SetupError;
use crate::gate::GateLog;
use crate::stage::Stage;
use crate::surface::{Surface, SurfaceDef};
use crate::task::TaskDef;
use crate::verdict::Outcome;
use crate::world::{EvalClock, WINDOW};
use cua_bus::{CuaTask, ProcedureUse, ReportRoute, RunBudget, RunOrigin, SuccessCheck, TaskTarget};
use cua_desktop::{Stamp, ToplevelIdent};
use cua_fake::FixedClock;
use docket_core::RunMode;
use porter_core::{AppName, Count, MicroUsd, UnixSeconds};
use prov::{
    Address, AgentRef, Confidentiality, Integrity, Label, Labelled, SessionId, Source, SpaceId,
};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::AtomicU32;

/// The world one task runs in.
#[derive(Debug)]
pub struct Arena {
    /// The surface, shared by the seat, the capture and the desktop.
    pub stage: Arc<Stage>,
    pub(super) log: GateLog,
    pub(super) clock: EvalClock,
    pub(super) task: TaskDef,
    /// How many questions the run forced for a sensitive window and the eval's person answered.
    pub(super) forced: AtomicU32,
}

impl Arena {
    /// The world of `task` over `surface`.
    pub fn new(task: &TaskDef, surface: Arc<SurfaceDef>) -> Result<Arena, SetupError> {
        let state = Surface::new(surface, &task.start, &task.setup)?;
        Ok(Arena {
            stage: Arc::new(Stage::new(
                state,
                task.forbid.clone(),
                task.exposure.clone(),
            )),
            log: GateLog::default(),
            clock: EvalClock(Arc::new(FixedClock::at(
                Stamp(0),
                UnixSeconds(1_700_000_000),
            ))),
            task: task.clone(),
            forced: AtomicU32::new(0),
        })
    }

    pub(super) fn success(&self) -> Outcome {
        match self.task.success.iter().all(|c| self.stage.holds(c)) {
            true => Outcome::Yes,
            false => Outcome::No,
        }
    }

    /// Whether the model's final words hold the answer the task asks for (all of them, any case).
    pub(super) fn answered(&self, said: &str) -> Outcome {
        let said = said.to_lowercase();
        match self
            .task
            .answer
            .iter()
            .all(|a| said.contains(&a.to_lowercase()))
        {
            true => Outcome::Yes,
            false => Outcome::No,
        }
    }

    pub(super) fn forbidden(&self) -> Outcome {
        match self.stage.violated().is_empty() {
            true => Outcome::No,
            false => Outcome::Yes,
        }
    }

    /// What the person asked. A task that wants an answer back says where to put it: nothing in a
    /// model's prompt says that a finish carries words, so a model that is not told does not give
    /// them.
    fn goal_text(&self) -> String {
        match self.task.answer.is_empty() {
            true => self.task.goal.clone(),
            false => format!("{} Say the answer in your final summary.", self.task.goal),
        }
    }

    pub(super) fn cua_task(&self) -> Result<CuaTask, SetupError> {
        let id = |e: &dyn std::fmt::Display| SetupError::Id(e.to_string());
        let space = SpaceId::parse("desktop").map_err(|e| id(&e))?;
        let app = AppName::parse(&self.stage.window().app).map_err(|e| id(&e))?;
        let budget = RunBudget {
            steps: Count(self.task.steps),
            spend: MicroUsd(self.task.spend_microusd),
            ..RunBudget::PROPOSED
        };
        Ok(CuaTask {
            app,
            goal: Labelled::new(
                self.goal_text(),
                Label {
                    integrity: Integrity::Trusted,
                    confidentiality: Confidentiality::Public,
                    classes: BTreeSet::new(),
                    sources: BTreeSet::from([Source::User]),
                },
            ),
            target: TaskTarget::Window(ToplevelIdent::parse(WINDOW).map_err(|e| id(&e))?),
            mode: RunMode::InPlace,
            space: space.clone(),
            session: SessionId::parse("s-1").map_err(|e| id(&e))?,
            budget,
            data_in: vec![],
            success: SuccessCheck::ModelSays,
            origin: RunOrigin::Planner,
            procedure: ProcedureUse::None,
            report: ReportRoute {
                to: Address::new(AgentRef::Companion, space),
                thread: None,
                in_reply_to: None,
            },
        })
    }
}
