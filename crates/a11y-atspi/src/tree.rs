//! The seam between the walk and the bus: what one accessible object says about itself.

use a11y_tree::A11yError;
use cua_desktop::Pid;
use std::future::Future;

/// Where an object is on the screen, as the toolkit reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Extents {
    /// The object has no `Component` interface, or did not answer.
    Unknown,
    /// Screen pixels.
    Screen {
        /// Left.
        x: i32,
        /// Top.
        y: i32,
        /// Width.
        w: i32,
        /// Height.
        h: i32,
    },
}

/// What one accessible object says about itself; `K` names an object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawNode<K> {
    /// Its role.
    pub role: ::atspi::Role,
    /// Its name.
    pub name: String,
    /// Its state bits.
    pub states: ::atspi::StateSet,
    /// Where it is.
    pub extents: Extents,
    /// The names of its actions.
    pub actions: Vec<String>,
    /// Its children, in order.
    pub children: Vec<K>,
}

/// The accessibility bus as the walk sees it: the applications on it and the objects below them.
pub trait AtspiTree: Send + Sync {
    /// Names one object.
    type Key: Clone + Send + Sync;

    /// The applications registered on the bus.
    fn applications(&self) -> impl Future<Output = Result<Vec<Self::Key>, A11yError>> + Send;

    /// The process an application runs in.
    fn process_of(&self, app: &Self::Key) -> impl Future<Output = Result<Pid, A11yError>> + Send;

    /// One object, with the names of its children.
    fn read(
        &self,
        node: &Self::Key,
    ) -> impl Future<Output = Result<RawNode<Self::Key>, A11yError>> + Send;
}
