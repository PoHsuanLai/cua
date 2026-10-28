//! Looking at the window: the tree, the frame, the messages that wait, and the success check.

use super::fit::{Fit, Refocus, Seen, Takes, fit_of};
use super::front::front_of;
use seat_input::AgentSeat;

use super::session::Following;
use super::{Exec, Outlet};
use crate::machine::effect::HintSource;
use crate::machine::input::{CheckResult, RunInput};
use crate::observation::{
    ContextObs, CursorObs, Focus, Observation, TreeAbsence, TreeObs, Visibility, WindowInfo,
};
use crate::screen::{Withheld, secret_masks, split_unseen};
use crate::seams::{Clock, MessageLink, Seams};
use crate::sensitivity::window_sensitivity;
use crate::turn::note_of;
use a11y_tree::{
    A11yError, A11ySnapshot, A11ySource, A11yTarget, NodeBudget, NodeSelector, Resolution, prune,
    resolve,
};
use cua_action::{Coord, Point, Rect, Size, WindowSpace};
use cua_bus::WindowRef;
use cua_desktop::{Desktop, LeaseControl, ToplevelInfo, ToplevelState, WorkspaceShown};
use docket_core::InboxAsk;
use maskcap::{CaptureError, CaptureSession, Frame, FrameWant, MaskReason, MaskRegion, WholeMask};
use porter_infer::StepIndex;
use prov::{AgentRef, Label, Labelled, Source};

/// How many times a window that is only not focused is read again before an action.
const FOCUS_TRIES: u32 = 10;
/// The pause between those reads: about half a second in all.
const FOCUS_POLL: std::time::Duration = std::time::Duration::from_millis(50);
/// How many times a frame is taken again after the dialog in front closed under it.
const DIALOG_RETRIES: u32 = 1;

/// What the first effects of a look found, kept until the capture builds the observation.
#[derive(Debug)]
pub(super) struct Staged {
    tree: TreeObs,
    withheld: Withheld,
    context: ContextObs,
}

impl Default for Staged {
    fn default() -> Self {
        Self {
            tree: TreeObs::Absent(TreeAbsence::Off),
            withheld: Withheld::default(),
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
    /// The window in front of the lease as the desktop reports it now (the leased window, or
    /// the dialog it opened: [`front_of`]); the lease's own record when it is not listed.
    pub(super) async fn window_now(&mut self) -> Option<ToplevelInfo> {
        let leased = self.held.as_ref()?.grant.window.clone();
        let all = self.seam.desktop.windows().await.unwrap_or_default();
        Some(front_of(&leased.ident, &all).cloned().unwrap_or(leased))
    }

    /// Keys for a leased window that does not have the focus: the desktop is first asked to
    /// focus it (casement allows that for a live lease, as the person's own run), and the window
    /// is read again. A backend that cannot, or refuses, leaves the window as it was and the
    /// short wait of [`Self::settled`] and its refusal stand.
    pub(super) async fn refocused(
        &mut self,
        now: ToplevelInfo,
        aimed: Size<WindowSpace>,
        takes: Takes,
    ) -> (ToplevelInfo, Refocus) {
        let isolation = self.seam.seat.caps().isolation;
        let lease = self.held.as_ref().map(|held| held.grant.id);
        let Some(lease) =
            lease.filter(|_| fit_of(&now, aimed, isolation, takes) == Fit::NotFocused)
        else {
            return (now, Refocus::Not);
        };
        if self.seam.desktop.focus(lease).await.is_err() {
            return (now, Refocus::Not);
        }
        match self.window_now().await {
            Some(again) => (again, Refocus::Asked),
            None => (now, Refocus::Not),
        }
    }

    /// The window and its fit for an action aimed at `aimed` that sends what `takes` says. A
    /// window that is only not focused is read again for a moment: right after a confirmation the focus is on its way back
    /// (the confirmation that took the person's answer is closing), so the action waits for it instead
    /// of being refused. It waits on the run's clock: a harness clock that never fires waits as
    /// long as it likes (an impatient one reads again at once).
    pub(super) async fn settled(
        &mut self,
        mut now: ToplevelInfo,
        aimed: Size<WindowSpace>,
        takes: Takes,
    ) -> (ToplevelInfo, Fit) {
        let isolation = self.seam.seat.caps().isolation;
        for _ in 0..FOCUS_TRIES {
            let fit = fit_of(&now, aimed, isolation, takes);
            if fit != Fit::NotFocused {
                return (now, fit);
            }
            self.seam.clock.after(FOCUS_POLL).await;
            match self.window_now().await {
                Some(again) => now = again,
                None => return (now, fit),
            }
        }
        let fit = fit_of(&now, aimed, isolation, takes);
        (now, fit)
    }

    /// The window's tree as the source gave it, cut to `budget`, with the window's size.
    async fn full_tree(
        &mut self,
        budget: NodeBudget,
    ) -> Result<(A11ySnapshot, Size<WindowSpace>), TreeAbsence> {
        if budget.0 == 0 {
            return Err(TreeAbsence::Off);
        }
        let Some(info) = self.window_now().await else {
            return Err(TreeAbsence::NoMatch);
        };
        let target = A11yTarget {
            pid: info.pid,
            app: info.app.clone(),
            title: info.title.clone(),
            geometry: size_of(&info),
            space: self.task.space.clone(),
        };
        match self.seam.a11y.snapshot(&target, budget).await {
            Ok(mut snapshot) => {
                // A source does not read the clock: the tree is stamped when the run got it.
                snapshot.taken = self.seam.clock.now();
                Ok((prune(&snapshot, budget), size_of(&info)))
            }
            Err(error) => Err(absence_of(&error)),
        }
    }

    /// The tree the model reads: [`Self::full_tree`] without the nodes the person cannot see.
    /// The only place that is done; the nodes left out go to the screen check.
    async fn tree_of(&mut self, budget: NodeBudget) -> (TreeObs, Withheld) {
        match self.full_tree(budget).await {
            Ok((snapshot, window)) => {
                let (seen, withheld) = split_unseen(snapshot, window);
                (TreeObs::Present(seen), withheld)
            }
            Err(absence) => (TreeObs::Absent(absence), Withheld::default()),
        }
    }

    pub(super) async fn snapshot(&mut self, budget: NodeBudget) {
        (self.staged.tree, self.staged.withheld) = self.tree_of(budget).await;
    }

    /// The masks the tree asks for: every secret node's rectangle, withheld nodes included.
    fn hints_of(&self, source: HintSource) -> Vec<MaskRegion> {
        match (source, &self.staged.tree) {
            (HintSource::FromTree, TreeObs::Present(snapshot)) => {
                secret_masks(snapshot, &self.staged.withheld)
                    .into_iter()
                    .map(|rect| MaskRegion {
                        rect,
                        why: MaskReason::A11ySecret,
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// The whole window as one mask, when it is sensitive and no accessibility tree says which
    /// parts are secret: with nothing to tell a password field from the rest, the safe default
    /// is to show the model none of it.
    fn blackout_of(&self, info: &ToplevelInfo) -> Option<MaskRegion> {
        match &self.staged.tree {
            TreeObs::Present(_) => None,
            TreeObs::Absent(_) => {
                window_sensitivity(self.seam.rules, &info.app, &info.title.0, &[])
                    .asks()
                    .then(|| MaskRegion {
                        rect: Rect {
                            origin: Point::new(Coord(0), Coord(0)),
                            size: size_of(info),
                        },
                        why: MaskReason::SensitiveWindow,
                    })
            }
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

    /// The window in front and a frame of it. A dialog that closed between the read and the
    /// frame is not the run's window going away: the window is read once more and the frame
    /// taken of what is in front then.
    async fn framed(
        &mut self,
        want: FrameWant,
        source: HintSource,
    ) -> Result<(ToplevelInfo, Frame, Option<MaskRegion>), CaptureError> {
        let mut again = DIALOG_RETRIES;
        loop {
            let info = self.window_now().await.ok_or(CaptureError::SourceGone)?;
            let mut hints = self.hints_of(source);
            let blackout = self.blackout_of(&info);
            hints.extend(blackout);
            let following = self.follow(&info).await?;
            let held = self.held.as_mut().ok_or(CaptureError::SourceGone)?;
            match held.session().frame(want, &hints).await {
                Ok(frame) => return Ok((info, frame, blackout)),
                Err(CaptureError::SourceGone) if following == Following::Dialog && again > 0 => {
                    again -= 1;
                    held.unfollow();
                }
                Err(error) => return Err(error),
            }
        }
    }

    pub(super) async fn capture(
        &mut self,
        want: FrameWant,
        hints: HintSource,
        out: &mut impl Outlet,
    ) {
        let (info, mut frame, blackout) = match self.framed(want, hints).await {
            Ok(framed) => framed,
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
            withheld: staged.withheld,
            context: staged.context,
            cursor: CursorObs::Unknown,
            prev: Vec::new(),
            notes: std::mem::take(&mut self.notes),
        })));
    }

    /// The success check on the tree: a fresh snapshot must have the node the selector names.
    pub(super) async fn verify(&mut self, selector: NodeSelector, out: &mut impl Outlet) {
        let budget = NodeBudget(u32::MAX);
        // The task's own check, not the model's view: it reads the whole tree.
        let result = match self.full_tree(budget).await {
            Ok((snapshot, _)) => match resolve(&snapshot, &selector) {
                Resolution::None => CheckResult::Fail,
                Resolution::One(_) | Resolution::Many(_) => CheckResult::Pass,
            },
            Err(_) => CheckResult::Fail,
        };
        out.input(RunInput::Checked(result));
    }
}
