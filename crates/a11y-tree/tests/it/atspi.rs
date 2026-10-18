//! The AT-SPI walk over an in-memory tree: the window is found by process and title, read
//! breadth first to the budget, in window-local pixels, with password fields marked secret. No
//! bus is touched.

use ::atspi::{Role as Atspi, State, StateSet};
use a11y_tree::{
    A11yError, A11yTarget, AtspiTree, Extents, NodeAction, NodeBudget, NodeState, Parent, RawNode,
    Role, Sensitivity, TreeSource, Truncation, read_window, secret_rects,
};
use cua_action::{Coord, Point, Rect, Size};
use cua_desktop::{Pid, WindowTitle};
use porter_core::{AppName, Count};
use prov::{Integrity, SpaceId};
use std::collections::HashMap;

/// Objects named by number: applications with their processes, and the objects themselves.
#[derive(Default)]
struct FakeTree {
    apps: Vec<(u32, u32)>,
    objects: HashMap<u32, RawNode<u32>>,
    bus: Option<A11yError>,
}

impl FakeTree {
    fn app(mut self, key: u32, pid: u32, windows: &[u32]) -> Self {
        self.apps.push((key, pid));
        self.put(key, Atspi::Application, "app", None, windows)
    }

    fn put(
        mut self,
        key: u32,
        role: Atspi,
        name: &str,
        at: Option<(i32, i32, i32, i32)>,
        children: &[u32],
    ) -> Self {
        let extents = at.map_or(Extents::Unknown, |(x, y, w, h)| Extents::Screen { x, y, w, h });
        self.objects.insert(
            key,
            RawNode {
                role,
                name: name.to_owned(),
                states: StateSet::from_iter([State::Enabled, State::Showing]),
                extents,
                actions: Vec::new(),
                children: children.to_vec(),
            },
        );
        self
    }

    fn acting(mut self, key: u32, action: &str) -> Self {
        if let Some(node) = self.objects.get_mut(&key) {
            node.actions.push(action.to_owned());
        }
        self
    }
}

impl AtspiTree for FakeTree {
    type Key = u32;

    async fn applications(&self) -> Result<Vec<u32>, A11yError> {
        match &self.bus {
            Some(error) => Err(error.clone()),
            None => Ok(self.apps.iter().map(|(key, _)| *key).collect()),
        }
    }

    async fn process_of(&self, app: &u32) -> Result<Pid, A11yError> {
        let found = self.apps.iter().find(|(key, _)| key == app);
        found.map(|(_, pid)| Pid(*pid)).ok_or(A11yError::Gone)
    }

    async fn read(&self, node: &u32) -> Result<RawNode<u32>, A11yError> {
        self.objects.get(node).cloned().ok_or(A11yError::Gone)
    }
}

fn target(pid: u32, title: &str, w: u32, h: u32) -> A11yTarget {
    A11yTarget {
        pid: Pid(pid),
        app: AppName::parse("org.example.editor").unwrap(),
        title: WindowTitle(title.to_owned()),
        geometry: Size::new(Coord(w), Coord(h)),
        space: SpaceId::parse("desktop").unwrap(),
    }
}

/// An editor at (100, 50) on the screen, 800 by 600: a tool bar with a Save button, and a
/// password field. A second editor window of the same process is smaller, and a window of
/// another process has the same title.
fn desktop() -> FakeTree {
    FakeTree::default()
        .app(1, 100, &[10, 11])
        .app(2, 200, &[20])
        .put(10, Atspi::Frame, "Editor", Some((100, 50, 800, 600)), &[100, 101])
        .put(11, Atspi::Frame, "Editor", Some((1000, 50, 400, 300)), &[])
        .put(20, Atspi::Frame, "Editor", Some((0, 0, 800, 600)), &[])
        .put(100, Atspi::Panel, "", Some((100, 50, 800, 40)), &[102])
        .put(101, Atspi::PasswordText, "Password", Some((200, 150, 300, 24)), &[])
        .put(102, Atspi::Button, "Save", Some((110, 55, 60, 30)), &[])
        .acting(102, "click")
}

fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect<cua_action::WindowSpace> {
    Rect {
        origin: Point::new(Coord(x), Coord(y)),
        size: Size::new(Coord(w), Coord(h)),
    }
}

#[tokio::test]
async fn the_window_is_read_breadth_first_in_window_local_pixels() {
    let snapshot = read_window(&desktop(), &target(100, "Editor", 800, 600), NodeBudget(400))
        .await
        .unwrap();
    assert_eq!(snapshot.source, TreeSource::Atspi);
    assert_eq!(snapshot.truncated, Truncation::Complete);
    let roles: Vec<Role> = snapshot.nodes.iter().map(|n| n.role).collect();
    assert_eq!(
        roles,
        [Role::Window, Role::Section, Role::PasswordText, Role::Button]
    );
    let parents: Vec<Parent> = snapshot.nodes.iter().map(|n| n.parent).collect();
    let id = |i: usize| snapshot.nodes[i].id;
    assert_eq!(
        parents,
        [
            Parent::Root,
            Parent::Node(id(0)),
            Parent::Node(id(0)),
            Parent::Node(id(1))
        ]
    );
    let save = &snapshot.nodes[3];
    assert_eq!(save.name.value, "Save");
    assert_eq!(save.name.label.integrity, Integrity::Untrusted);
    assert_eq!(save.bounds, rect(10, 5, 60, 30));
    assert_eq!(save.actions, [NodeAction::Click]);
    assert!(save.states.contains(&NodeState::Enabled));
    assert!(save.states.contains(&NodeState::Showing));
    assert_eq!(snapshot.nodes[0].bounds, rect(0, 0, 800, 600));
}

#[tokio::test]
async fn a_budget_cuts_the_walk_and_says_how_much_was_left() {
    let snapshot = read_window(&desktop(), &target(100, "Editor", 800, 600), NodeBudget(2))
        .await
        .unwrap();
    assert_eq!(snapshot.nodes.len(), 2);
    assert_eq!(
        snapshot.truncated,
        Truncation::Pruned { dropped: Count(2) }
    );
}

#[tokio::test]
async fn a_password_field_is_secret_and_its_rectangle_is_a_mask() {
    let snapshot = read_window(&desktop(), &target(100, "Editor", 800, 600), NodeBudget(400))
        .await
        .unwrap();
    let secret: Vec<&str> = snapshot
        .nodes
        .iter()
        .filter(|n| n.sensitivity == Sensitivity::Secret)
        .map(|n| n.name.value.as_str())
        .collect();
    assert_eq!(secret, ["Password"]);
    assert_eq!(secret_rects(&snapshot), [rect(100, 100, 300, 24)]);
}

#[tokio::test]
async fn windows_are_told_apart_by_process_title_and_size() {
    let budget = NodeBudget(400);
    // The smaller editor window of the same process.
    let small = read_window(&desktop(), &target(100, "Editor", 400, 300), budget)
        .await
        .unwrap();
    assert_eq!(small.nodes.len(), 1);
    assert_eq!(small.nodes[0].bounds, rect(0, 0, 400, 300));
    // The other process's window of the same title is not this one.
    let other = read_window(&desktop(), &target(200, "Editor", 800, 600), budget)
        .await
        .unwrap();
    assert_eq!(other.nodes.len(), 1);
    // A desktop that names no process still finds a window that title and size single out.
    let unnamed = read_window(&desktop(), &target(0, "Editor", 400, 300), budget).await;
    assert_eq!(unnamed.unwrap().nodes[0].bounds, rect(0, 0, 400, 300));
    let both = read_window(&desktop(), &target(0, "Editor", 800, 600), budget).await;
    assert_eq!(both, Err(A11yError::NoMatchingWindow));
    // A title that changed since the lease still finds the one window that has its size.
    let renamed = read_window(&desktop(), &target(100, "Editor *", 800, 600), budget).await;
    assert_eq!(renamed.unwrap().nodes.len(), 4);
    // Equal title and equal size are ambiguous.
    let twins = desktop().put(11, Atspi::Frame, "Editor", Some((1000, 50, 800, 600)), &[]);
    assert_eq!(
        read_window(&twins, &target(100, "Editor", 800, 600), budget).await,
        Err(A11yError::NoMatchingWindow)
    );
}

#[tokio::test]
async fn no_tree_is_a_typed_error_and_never_a_panic() {
    let budget = NodeBudget(400);
    let nobody = target(999, "Editor", 800, 600);
    assert_eq!(
        read_window(&desktop(), &nobody, budget).await,
        Err(A11yError::NotExposed)
    );
    let no_bus = FakeTree {
        bus: Some(A11yError::NoBus),
        ..FakeTree::default()
    };
    assert_eq!(
        read_window(&no_bus, &target(100, "Editor", 800, 600), budget).await,
        Err(A11yError::NoBus)
    );
    let mut vanished = desktop();
    vanished.objects.remove(&10);
    vanished.objects.remove(&11);
    assert_eq!(
        read_window(&vanished, &target(100, "Editor", 800, 600), budget).await,
        Err(A11yError::NoMatchingWindow)
    );
}
