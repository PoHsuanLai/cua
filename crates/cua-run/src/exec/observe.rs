//! Looking at the window: the tree, the frame, the messages that wait, and the success check.

use super::fit::Seen;
use super::{Exec, Outlet};
use crate::machine::effect::HintSource;
use crate::machine::input::{CheckResult, RunInput};
use crate::observation::{
    ContextObs, CursorObs, Focus, Observation, TreeAbsence, TreeObs, Visibility, WindowInfo,
};
use crate::seams::{MessageLink, Seams};
use crate::sensitivity::window_sensitivity;
use crate::turn::note_of;
use a11y_tree::{
    A11yError, A11ySource, A11yTarget, NodeBudget, NodeSelector, Resolution, prune, resolve,
    secret_rects,
};
use cua_action::{Coord, Point, Rect, Size, WindowSpace};
use cua_bus::WindowRef;
use cua_desktop::{Desktop, ToplevelInfo, ToplevelState, WorkspaceShown};
use docket_core::InboxAsk;
use maskcap::{CaptureError, CaptureSession, FrameWant, MaskReason, MaskRegion, WholeMask};
use porter_infer::StepIndex;
use prov::{AgentRef, Label, Labelled, Source};

/// What the first effects of a look found, kept until the capture builds the observation.
#[derive(Debug)]
pub(super) struct Staged {
    tree: TreeObs,
    context: ContextObs,
}

impl Default for Staged {
    fn default() -> Self {
        Self {
            tree: TreeObs::Absent(TreeAbsence::Off),
            context: ContextObs::Absent,
        }
    }
}

impl Staged {
    /// The app was not asked: no seam carries `Context` to cuad yet.
    pub(super) fn no_context(&mut self) {
        self.context = ContextObs::Absent;
    }
}

fn absence_of(error: &A11yError) -> TreeAbsence {
    match error {
        A11yError::NoBus
        | A11yError::NotExposed
        | A11yError::Protocol(_)
        | A11yError::BackendUnavailable => TreeAbsence::NotExposed,
        A11yError::NoMatchingWindow | A11yError::Gone => TreeAbsence::NoMatch,
        A11yError::Timeout => TreeAbsence::Timeout,
    }
}

fn size_of(info: &ToplevelInfo) -> Size<WindowSpace> {
    Size::new(Coord(info.geometry.w.0), Coord(info.geometry.h.0))
}

pub(super) fn visibility_of(info: &ToplevelInfo) -> Visibility {
    if info.state.contains(&ToplevelState::Minimized) {
        Visibility::Minimized
    } else if info.workspace.shown == WorkspaceShown::Hidden {
        Visibility::HiddenWorkspace
    } else {
        Visibility::Shown
    }
}

impl<S: Seams> Exec<'_, S> {
    /// The window as the desktop reports it now; the lease's own record when it is not listed.
    pub(super) async fn window_now(&mut self) -> Option<ToplevelInfo> {
        let leased = self.held.as_ref()?.grant.window.clone();
        let listed = self
            .seam
            .desktop
            .windows()
            .await
            .ok()
            .and_then(|all| all.into_iter().find(|w| w.ident == leased.ident));
        listed.or(Some(leased))
    }

    async fn tree_of(&mut self, budget: NodeBudget) -> TreeObs {
        if budget.0 == 0 {
            return TreeObs::Absent(TreeAbsence::Off);
        }
        let Some(info) = self.window_now().await else {
            return TreeObs::Absent(TreeAbsence::NoMatch);
        };
        let target = A11yTarget {
            pid: info.pid,
            app: info.app.clone(),
            title: info.title.clone(),
            geometry: size_of(&info),
        };
        match self.seam.a11y.snapshot(&target, budget).await {
            Ok(snapshot) => TreeObs::Present(prune(&snapshot, budget)),
            Err(error) => TreeObs::Absent(absence_of(&error)),
        }
    }

    pub(super) async fn snapshot(&mut self, budget: NodeBudget) {
        self.staged.tree = self.tree_of(budget).await;
    }

    /// The masks the tree asks for: every secret node's rectangle.
    fn hints_of(&self, source: HintSource) -> Vec<MaskRegion> {
        match (source, &self.staged.tree) {
            (HintSource::FromTree, TreeObs::Present(snapshot)) => secret_rects(snapshot)
                .into_iter()
                .map(|rect| MaskRegion {
                    rect,
                    why: MaskReason::A11ySecret,
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    /// The whole window as one mask, when it is sensitive and no accessibility tree says which
    /// parts are secret: with nothing to tell a password field from the rest, the safe default
    /// is to show the model none of it.
    fn blackout_of(&self, info: &ToplevelInfo) -> Option<MaskRegion> {
        match &self.staged.tree {
            TreeObs::Present(_) => None,
            TreeObs::Absent(_) => window_sensitivity(&info.app, &info.title.0, &[])
                .asks()
                .then(|| MaskRegion {
                    rect: Rect {
                        origin: Point::new(Coord(0), Coord(0)),
                        size: size_of(info),
                    },
                    why: MaskReason::SensitiveWindow,
                }),
        }
    }

    /// Takes the messages that wait for the run: each is an input to the machine (which
    /// changes nothing by it) and a note for the model's next turn.
    async fn read_inbox(&mut self, out: &mut impl Outlet) {
        let ask = InboxAsk {
            agent: AgentRef::Cua {
                run: self.run.clone(),
            },
            after: self.last_message.clone(),
        };
        let Ok(lines) = self.seam.events.inbox(ask).await else {
            return;
        };
        for line in lines {
            self.last_message = Some(line.id.clone());
            self.notes.extend(note_of(&line));
            out.input(RunInput::Inbound(Box::new(line)));
        }
    }

    pub(super) async fn capture(
        &mut self,
        want: FrameWant,
        hints: HintSource,
        out: &mut impl Outlet,
    ) {
        let Some(info) = self.window_now().await else {
            return out.input(RunInput::CaptureFailed(CaptureError::SourceGone));
        };
        let mut hints = self.hints_of(hints);
        let blackout = self.blackout_of(&info);
        hints.extend(blackout);
        let Some(held) = self.held.as_mut() else {
            return out.input(RunInput::CaptureFailed(CaptureError::SourceGone));
        };
        let mut frame = match held.capture.frame(want, &hints).await {
            Ok(frame) => frame,
            Err(error) => return out.input(RunInput::CaptureFailed(error)),
        };
        if blackout.is_some() {
            frame.masks.whole = WholeMask::Blacked(MaskReason::SensitiveWindow);
        }
        self.read_inbox(out).await;
        self.seen = Some(Seen::of(&info));
        let label = Label::untrusted(
            Source::Screen {
                app: info.app.clone(),
            },
            porter_core::DataClass::Screen,
            self.task.space.clone(),
        );
        let staged = std::mem::take(&mut self.staged);
        out.input(RunInput::Observed(Box::new(Observation {
            step: StepIndex(0),
            window: WindowInfo {
                window: WindowRef(1),
                app: info.app.clone(),
                title: Labelled::new(info.title.0.clone(), label),
                size: size_of(&info),
                scale: frame.scale,
                focus: if info.state.contains(&ToplevelState::Activated) {
                    Focus::Focused
                } else {
                    Focus::Unfocused
                },
                visibility: visibility_of(&info),
                trust: info.trust,
            },
            frame,
            tree: staged.tree,
            context: staged.context,
            cursor: CursorObs::Unknown,
            prev: Vec::new(),
            notes: std::mem::take(&mut self.notes),
        })));
    }

    /// The success check on the tree: a fresh snapshot must have the node the selector names.
    pub(super) async fn verify(&mut self, selector: NodeSelector, out: &mut impl Outlet) {
        let budget = NodeBudget(u32::MAX);
        let result = match self.tree_of(budget).await {
            TreeObs::Present(snapshot) => match resolve(&snapshot, &selector) {
                Resolution::None => CheckResult::Fail,
                Resolution::One(_) | Resolution::Many(_) => CheckResult::Pass,
            },
            TreeObs::Absent(_) => CheckResult::Fail,
        };
        out.input(RunInput::Checked(result));
    }
}
