//! Finding the leased window on the bus and walking its tree, breadth first, to the budget.

use super::map;
use super::tree::{AtspiTree, Extents, RawNode};
use crate::model::{
    A11yNode, A11ySnapshot, NodeBudget, NodeValue, Parent, Sensitivity, TreeSource, Truncation,
    TypedHint,
};
use crate::source::{A11yError, A11yTarget};
use cua_action::{Coord, NodeId, Point, Rect, Size, WindowSpace};
use cua_desktop::{Pid, Stamp};
use porter_core::{Count, DataClass};
use prov::{Label, Labelled, Source};
use std::collections::VecDeque;

/// The window of `target` on `tree`, at most `budget` nodes of it. The snapshot's `taken` is
/// zero: a source reads no clock, and whoever asked stamps it.
///
/// The window is the child of an application of the target's process (any application when the
/// desktop named none, pid 0) whose name is the target's title; windows of equal title are told apart by size, and what is still ambiguous is
/// [`A11yError::NoMatchingWindow`]. A process with no application on the bus is
/// [`A11yError::NotExposed`].
pub async fn read_window<T: AtspiTree>(
    tree: &T,
    target: &A11yTarget,
    budget: NodeBudget,
) -> Result<A11ySnapshot, A11yError> {
    let window = find_window(tree, target).await?;
    walk(tree, window, target, budget).await
}

async fn find_window<T: AtspiTree>(tree: &T, target: &A11yTarget) -> Result<T::Key, A11yError> {
    let mut exposed = false;
    let mut windows = Vec::new();
    for app in tree.applications().await? {
        match target.pid {
            // The desktop named no process (stock's protocols do not): any application will do,
            // and title and size tell the windows apart.
            Pid(0) => {}
            // An application that went away while we looked is not the one we want.
            pid if tree.process_of(&app).await != Ok(pid) => continue,
            Pid(_) => {}
        }
        exposed = true;
        let Ok(app_node) = tree.read(&app).await else {
            continue;
        };
        for child in app_node.children {
            if let Ok(node) = tree.read(&child).await {
                windows.push((child, node));
            }
        }
    }
    if !exposed {
        return Err(A11yError::NotExposed);
    }
    let titled = narrow(windows, |(_, node)| node.name == target.title.0);
    let sized = narrow(titled, |(_, node)| fits(node.extents, target));
    match <[_; 1]>::try_from(sized) {
        Ok([(key, _)]) => Ok(key),
        Err(_) => Err(A11yError::NoMatchingWindow),
    }
}

/// Those of `items` that `keep` keeps; all of them when it keeps none.
fn narrow<I>(items: Vec<I>, keep: impl Fn(&I) -> bool) -> Vec<I> {
    if items.iter().any(&keep) {
        items.into_iter().filter(keep).collect()
    } else {
        items
    }
}

fn fits(extents: Extents, target: &A11yTarget) -> bool {
    match extents {
        Extents::Unknown => false,
        Extents::Screen { w, h, .. } => {
            u32::try_from(w) == Ok(target.geometry.w.0)
                && u32::try_from(h) == Ok(target.geometry.h.0)
        }
    }
}

async fn walk<T: AtspiTree>(
    tree: &T,
    window: T::Key,
    target: &A11yTarget,
    budget: NodeBudget,
) -> Result<A11ySnapshot, A11yError> {
    let limit = usize::try_from(budget.0).unwrap_or(usize::MAX).max(1);
    let label = Label::untrusted(
        Source::Screen {
            app: target.app.clone(),
        },
        DataClass::Screen,
        target.space.clone(),
    );
    let mut origin = (0, 0);
    let mut nodes: Vec<A11yNode> = Vec::new();
    let mut queue = VecDeque::from([(window, Parent::Root)]);
    while nodes.len() < limit
        && let Some((key, parent)) = queue.pop_front()
    {
        let raw = match tree.read(&key).await {
            Ok(raw) => raw,
            // The window itself is gone; a node below it that went away is just left out.
            Err(error) if nodes.is_empty() => return Err(error),
            Err(_) => continue,
        };
        if nodes.is_empty() {
            origin = origin_of(raw.extents);
        }
        let id = NodeId(u32::try_from(nodes.len()).unwrap_or(u32::MAX));
        queue.extend(raw.children.iter().map(|c| (c.clone(), Parent::Node(id))));
        nodes.push(node_of(id, parent, raw, origin, &label));
    }
    let left = u32::try_from(queue.len()).unwrap_or(u32::MAX);
    Ok(A11ySnapshot {
        source: TreeSource::Atspi,
        taken: Stamp(0),
        nodes,
        truncated: match left {
            0 => Truncation::Complete,
            dropped => Truncation::Pruned {
                dropped: Count(dropped),
            },
        },
    })
}

fn origin_of(extents: Extents) -> (i32, i32) {
    match extents {
        Extents::Unknown => (0, 0),
        Extents::Screen { x, y, .. } => (x, y),
    }
}

fn node_of<K>(
    id: NodeId,
    parent: Parent,
    raw: RawNode<K>,
    origin: (i32, i32),
    label: &Label,
) -> A11yNode {
    let role = map::role(raw.role);
    A11yNode {
        id,
        parent,
        role,
        name: Labelled::new(raw.name, label.clone()),
        value: NodeValue::None,
        states: map::states(raw.states),
        bounds: local(raw.extents, origin),
        actions: raw.actions.iter().map(|a| map::action(a)).collect(),
        typed: TypedHint::None,
        sensitivity: match role {
            crate::model::Role::PasswordText => Sensitivity::Secret,
            _ => Sensitivity::Normal,
        },
    }
}

/// Screen extents as window-local pixels; what lies left of or above the window is cut to its
/// edge, and an object with no extents is a zero rectangle.
fn local(extents: Extents, origin: (i32, i32)) -> Rect<WindowSpace> {
    let px = |v: i32| Coord(u32::try_from(v.max(0)).unwrap_or(0));
    match extents {
        Extents::Unknown => Rect {
            origin: Point::new(Coord(0), Coord(0)),
            size: Size::new(Coord(0), Coord(0)),
        },
        Extents::Screen { x, y, w, h } => Rect {
            origin: Point::new(
                px(x.saturating_sub(origin.0)),
                px(y.saturating_sub(origin.1)),
            ),
            size: Size::new(px(w), px(h)),
        },
    }
}
