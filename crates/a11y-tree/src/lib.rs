//! The accessibility tree of a leased window.
//!
//! A snapshot ([`A11ySnapshot`]) is an arena of nodes with window-local bounds. Names and text
//! values are screen content and carry an `Untrusted(Screen)` label. The model sees the tree as
//! text ([`render`]), cut to a budget ([`prune`]); secret nodes (password entries, quire-declared
//! sensitive controls) are never rendered and their rectangles ([`secret_rects`]) become mask
//! hints for `maskcap`.
//!
//! Pure by default. The `atspi` feature is the AT-SPI backend ([`AtspiSource`]): it finds the
//! leased window by process and title on the accessibility bus and walks it breadth first to the
//! node budget. A missing bus, an app with no tree and a slow bus are [`A11yError`] values, and a
//! run goes on from pixels alone.

#[cfg(feature = "atspi")]
mod atspi;
mod model;
mod ops;
mod render;
mod selector;
mod source;

#[cfg(feature = "atspi")]
pub use atspi::{AtspiSource, AtspiTree, ConnectionHeld, Extents, RawNode, read_window};
pub use model::{
    A11yNode, A11ySnapshot, NodeAction, NodeBudget, NodeState, NodeValue, Parent, Role,
    Sensitivity, TreeSource, Truncation, TypedHint,
};
pub use ops::{TreeDiff, diff, prune, render, resolve, secret_rects, secret_rects_of};
pub use render::role_word;
pub use selector::{NameMatch, NodeSelector, Resolution, SelectorStep, SlotName};
pub use source::{A11yError, A11ySource, A11yTarget};
