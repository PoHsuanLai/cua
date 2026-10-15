//! The small moves the transition rows are made of: entering a phase, starting a look at the
//! window, recording a finished action, asking the router about the head action.

use crate::effect_class::classify_effect_in;
use crate::machine::effect::{CuaSignal, HintSource, RunEffect};
use crate::machine::kept::kept_action;
use crate::machine::phase::RunPhase;
use crate::machine::run::Run;
use crate::observation::{Observation, TreeObs};
use crate::window_class::window_class_in;
use a11y_tree::{A11yNode, role_word};
use cua_action::{Coord, CuaAction, Point, Rect, Size, Target, WindowSpace};
use cua_bus::{ActionOutcome, ActionResult, CuaRecord, FrameNote, TargetedAction, ThoughtDigest};
use cua_desktop::{Digest32, GlowState, Stamp};
use docket_core::{CuaAsk, NodeFacts};
use maskcap::FrameWant;
use porter_core::DataClass;
use porter_infer::StepIndex;
use prov::{Label, Source};
use seat_input::{InputReceipt, PlannedAction, ResolvedPoints, centre_of};

/// Enters `phase`, telling watchers when the slug changes.
pub(crate) fn enter(run: &mut Run, fx: &mut Vec<RunEffect>, phase: RunPhase) {
    if run.phase.slug() != phase.slug() {
        fx.push(RunEffect::Emit(CuaSignal::StateChanged(phase.slug())));
    }
    run.phase = phase;
}

/// Whether the look at the window also asks the app what it says about where the person is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AlsoContext {
    /// The first look of a run (or after the person had the window).
    Yes,
    /// A later one.
    No,
}

/// Starts step `n`: the tree first (so the masks come from the tree of this very frame), the
/// app's context if asked, and the capture last: the capture is what builds the observation, so
/// it is always the final effect of a look.
pub(crate) fn observe(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    n: StepIndex,
    want: FrameWant,
    context: AlsoContext,
) {
    let new_step = run.phase.step() != Some(n) || !matches!(run.phase, RunPhase::Observing(_));
    enter(run, fx, RunPhase::Observing(n));
    if new_step {
        fx.push(RunEffect::Emit(CuaSignal::StepStarted(n)));
    }
    fx.push(RunEffect::Snapshot(run.env.tree_max_nodes));
    if context == AlsoContext::Yes {
        fx.push(RunEffect::FetchContext);
    }
    fx.push(RunEffect::Capture {
        want,
        hints: HintSource::FromTree,
    });
    run.track.capture = crate::machine::track::CaptureTries::First;
}

/// The next step's index.
pub(crate) fn next(n: StepIndex) -> StepIndex {
    StepIndex(n.0.saturating_add(1))
}

/// The label of whatever is read off the leased window.
pub(crate) fn screen_label(run: &Run) -> Label {
    Label::untrusted(
        Source::Screen {
            app: run.task.app.clone(),
        },
        DataClass::Screen,
        run.task.space.clone(),
    )
}

/// The current step's index, or the first when there is none yet.
pub(crate) fn step_of(run: &Run) -> StepIndex {
    run.phase.step().unwrap_or(StepIndex(0))
}

/// The window the run holds, as an action's target.
pub(crate) fn window_ref(run: &Run) -> cua_bus::WindowRef {
    run.working
        .as_ref()
        .map_or(cua_bus::WindowRef(1), |o| o.window.window)
}

/// An action of the model, aimed at the window.
pub(crate) fn targeted(run: &Run, action: CuaAction<WindowSpace>) -> TargetedAction {
    TargetedAction {
        window: window_ref(run),
        action,
    }
}

/// An outcome for an action the model did not name (the person took the window, a question was
/// answered): attached to a plain look.
pub(crate) fn synthetic(run: &Run, result: ActionResult) -> ActionOutcome {
    ActionOutcome {
        action: targeted(run, CuaAction::Observe),
        result,
    }
}

/// The hash of a thought: the text is never kept.
pub(crate) fn thought_digest(text: &str) -> ThoughtDigest {
    ThoughtDigest(Digest32(*blake3::hash(text.as_bytes()).as_bytes()))
}

fn node_with(snapshot_nodes: &[A11yNode], id: cua_action::NodeId) -> Option<&A11yNode> {
    snapshot_nodes.iter().find(|n| n.id == id)
}

/// The tree of the step's observation, if there is one.
pub(crate) fn nodes_of(obs: &Observation) -> &[A11yNode] {
    match &obs.tree {
        TreeObs::Present(snapshot) => &snapshot.nodes,
        TreeObs::Absent(_) => &[],
    }
}

fn contains(rect: &Rect<WindowSpace>, p: Point<WindowSpace>) -> bool {
    let (x0, y0) = (rect.origin.x.0, rect.origin.y.0);
    p.x.0 >= x0
        && p.y.0 >= y0
        && p.x.0 < x0.saturating_add(rect.size.w.0)
        && p.y.0 < y0.saturating_add(rect.size.h.0)
}

fn area(rect: &Rect<WindowSpace>) -> u64 {
    u64::from(rect.size.w.0) * u64::from(rect.size.h.0)
}

/// The node a target touches: the one it names, or the smallest one on screen that holds the
/// point.
pub(crate) fn node_at<'a>(
    obs: &'a Observation,
    target: &Target<WindowSpace>,
) -> Option<&'a A11yNode> {
    let nodes = nodes_of(obs);
    match target {
        Target::Node(id) => node_with(nodes, *id),
        Target::Point(p) => nodes
            .iter()
            .filter(|n| contains(&n.bounds, *p))
            .min_by_key(|n| area(&n.bounds)),
        Target::Centre => None,
    }
}

/// The first target of a pointer action.
pub(crate) fn first_target(action: &CuaAction<WindowSpace>) -> Option<Target<WindowSpace>> {
    match action {
        CuaAction::Click { at, .. } | CuaAction::MoveTo { at } | CuaAction::Scroll { at, .. } => {
            Some(*at)
        }
        CuaAction::Drag { from, .. } => Some(*from),
        _ => None,
    }
}

/// The centre of a node's bounds, clamped inside the window.
fn centre_inside(rect: &Rect<WindowSpace>, window: Size<WindowSpace>) -> Point<WindowSpace> {
    let clamp = |v: u32, limit: u32| Coord(v.min(limit.saturating_sub(1)));
    Point::new(
        clamp(
            rect.origin.x.0.saturating_add(rect.size.w.0 / 2),
            window.w.0,
        ),
        clamp(
            rect.origin.y.0.saturating_add(rect.size.h.0 / 2),
            window.h.0,
        ),
    )
}

/// Why an action cannot be aimed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Unaimed {
    /// A point outside the window.
    Outside,
    /// A node the tree does not have.
    NoSuchNode,
}

fn resolve_target(
    obs: &Observation,
    target: &Target<WindowSpace>,
) -> Result<Option<Point<WindowSpace>>, Unaimed> {
    let size = obs.window.size;
    match target {
        Target::Centre => Ok(None),
        Target::Point(p) => {
            if p.x.0 < size.w.0 && p.y.0 < size.h.0 {
                Ok(Some(*p))
            } else {
                Err(Unaimed::Outside)
            }
        }
        Target::Node(id) => node_with(nodes_of(obs), *id)
            .map(|n| Some(centre_inside(&n.bounds, size)))
            .ok_or(Unaimed::NoSuchNode),
    }
}

/// The action ready to plan: its targets resolved against this step's tree.
pub(crate) fn aim(
    obs: &Observation,
    action: &CuaAction<WindowSpace>,
) -> Result<PlannedAction, Unaimed> {
    let at = match action {
        CuaAction::Click { at, .. } | CuaAction::MoveTo { at } | CuaAction::Scroll { at, .. } => {
            resolve_target(obs, at)?.map_or(ResolvedPoints::None, ResolvedPoints::One)
        }
        CuaAction::Drag { from, to, .. } => {
            let centre = centre_of(obs.window.size);
            ResolvedPoints::Drag {
                from: resolve_target(obs, from)?.unwrap_or(centre),
                to: resolve_target(obs, to)?.unwrap_or(centre),
            }
        }
        _ => ResolvedPoints::None,
    };
    Ok(PlannedAction {
        action: action.clone(),
        at,
        window: obs.window.size,
    })
}

/// What the glow shows while `planned` is carried out.
pub(crate) fn glow_for(planned: &PlannedAction) -> GlowState {
    let at = match planned.at {
        ResolvedPoints::One(p) => Some(p),
        ResolvedPoints::Drag { to, .. } => Some(to),
        ResolvedPoints::None => None,
    };
    match at {
        Some(origin) => GlowState::Acting {
            at: Rect {
                origin,
                size: Size::new(Coord(1), Coord(1)),
            },
        },
        None => GlowState::Working,
    }
}

/// The question to the router about the head action.
pub(crate) fn ask_for(run: &Run, obs: &Observation, action: &CuaAction<WindowSpace>) -> CuaAsk {
    let node = first_target(action).and_then(|t| node_at(obs, &t));
    let class = window_class_in(&obs.window.app, &obs.window.title.value, nodes_of(obs));
    let (effect, basis) = classify_effect_in(run.env.keys, action, node, class);
    CuaAsk {
        run: run.id.clone(),
        step: obs.step.0,
        app: obs.window.app.clone(),
        trust: obs.window.trust,
        mode: run.task.mode,
        space: run.task.space.clone(),
        action: action.clone(),
        node: node.map(|n| NodeFacts {
            role: role_word(n.role),
            name: n.name.clone(),
        }),
        effect,
        basis,
        screen: screen_label(run),
    }
}

/// The record of one action that ended, with the effect it was classed as.
pub(crate) fn step_record(
    run: &Run,
    outcome: &ActionOutcome,
    input: Option<InputReceipt>,
) -> RunEffect {
    let (effect, basis) = match &run.working {
        Some(obs) => {
            let node = first_target(&outcome.action.action).and_then(|t| node_at(obs, &t));
            let class = window_class_in(&obs.window.app, &obs.window.title.value, nodes_of(obs));
            classify_effect_in(run.env.keys, &outcome.action.action, node, class)
        }
        None => classify_effect_in(
            run.env.keys,
            &outcome.action.action,
            None,
            cua_action::WindowClass::Ordinary,
        ),
    };
    RunEffect::Record(Box::new(CuaRecord::Step {
        run: run.id.clone(),
        n: step_of(run),
        thought: run
            .track
            .thought
            .unwrap_or(ThoughtDigest(Digest32([0; 32]))),
        action: kept_action(&outcome.action),
        effect,
        basis,
        authorised_by: run.track.authorised,
        result: outcome.result.clone(),
        frame: run.track.frame.clone().unwrap_or(FrameNote::NotKept {
            digest: maskcap::FrameDigest(Digest32([0; 32])),
            size: cua_action::DeviceSize { w: 0, h: 0 },
        }),
        input,
    }))
}

/// Notes an action's end: its outcome joins the step's, the watchers hear of it, and the log
/// gets its record.
pub(crate) fn action_ended(
    run: &mut Run,
    fx: &mut Vec<RunEffect>,
    outcome: ActionOutcome,
    input: Option<InputReceipt>,
) {
    fx.push(step_record(run, &outcome, input));
    fx.push(RunEffect::Emit(CuaSignal::ActionDone(
        step_of(run),
        outcome.clone(),
    )));
    run.track.outcomes.push(outcome);
}

/// The rest of the batch becomes `NotExecuted`: an action that did not run, or ran and failed,
/// takes the actions after it with it.
pub(crate) fn drop_rest(run: &mut Run) {
    let dropped = std::mem::take(&mut run.pending.actions);
    run.track
        .outcomes
        .extend(dropped.into_iter().map(|action| ActionOutcome {
            action,
            result: ActionResult::NotExecuted,
        }));
}

/// Active time up to `at`: counted between instants, only in an active phase.
pub(crate) fn count_active(run: &mut Run, at: Stamp) {
    if run.phase.is_active() {
        let spent = at.0.saturating_sub(run.last_at.0);
        let ms = u32::try_from(spent).unwrap_or(u32::MAX);
        run.used.active = cua_desktop::Ms(run.used.active.0.saturating_add(ms));
    }
    run.last_at = at;
}
