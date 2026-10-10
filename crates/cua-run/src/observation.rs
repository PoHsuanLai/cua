//! What a run sees at the start of a step.

use a11y_tree::A11ySnapshot;
use cua_action::WindowTrust;
use cua_action::{Point, Scale120, Size, WindowSpace};
use cua_bus::{ActionOutcome, WindowRef};
use docket_core::ContextSnapshot;
use maskcap::Frame;
use porter_core::AppName;
use porter_infer::{StepIndex, StepNote};
use prov::Labelled;
use serde::{Deserialize, Serialize};

/// Whether the window has keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Focus {
    /// It has.
    Focused,
    /// It has not.
    Unfocused,
}

/// Whether the window can be seen and reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// On screen.
    Shown,
    /// Covered by other windows (capture still shows it clean).
    Occluded,
    /// On a workspace that is not shown: captured, but on stock it takes no input.
    HiddenWorkspace,
    /// Minimized.
    Minimized,
}

/// The leased window as this step finds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInfo {
    /// The run's handle for it.
    pub window: WindowRef,
    /// The app.
    pub app: AppName,
    /// The title: screen text, labelled untrusted.
    pub title: Labelled<String>,
    /// Logical size.
    pub size: Size<WindowSpace>,
    /// Device pixels per logical pixel in 120ths (120 on stock at any output scale).
    pub scale: Scale120,
    /// Focus.
    pub focus: Focus,
    /// Visibility.
    pub visibility: Visibility,
    /// How far the window is trusted.
    pub trust: WindowTrust,
}

/// Why there is no tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TreeAbsence {
    /// The app does not expose one.
    NotExposed,
    /// No unambiguous matching window.
    NoMatch,
    /// Too slow.
    Timeout,
    /// The setting is off.
    Off,
}

/// The accessibility tree, if there is one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TreeObs {
    /// None, and why.
    Absent(TreeAbsence),
    /// A snapshot.
    Present(A11ySnapshot),
}

/// What the app says about where the person is, if it answers `Context`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ContextObs {
    /// The app did not answer.
    Absent,
    /// Its snapshot (its fields carry their own labels).
    Present(Box<ContextSnapshot>),
}

/// Where the agent's pointer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CursorObs {
    /// Not known yet.
    Unknown,
    /// Here, window-local.
    At(Point<WindowSpace>),
}

/// Everything a step starts from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Observation {
    /// Which step.
    pub step: StepIndex,
    /// The window.
    pub window: WindowInfo,
    /// The masked frame.
    pub frame: Frame,
    /// The tree.
    pub tree: TreeObs,
    /// The context.
    pub context: ContextObs,
    /// The agent's pointer.
    pub cursor: CursorObs,
    /// How the previous step's actions ended.
    pub prev: Vec<ActionOutcome>,
    /// The messages that reached the run since the last step, as the model reads them, oldest
    /// first (the supervisor builds them from `RunInput::Inbound`). Text only: a note is
    /// untrusted words or the person's own, and never authority.
    pub notes: Vec<StepNote>,
}
