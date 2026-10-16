//! The screen check: does what the model is about to be shown hold text aimed at it?
//!
//! cua labels everything it reads off the screen `Untrusted(Screen)` and shows it as data; this
//! is the stage that *looks*. Each turn, before the model is called, the run machine asks a
//! [`ScreenCheck`] about the window's title and its accessibility tree (the text the model will
//! read). The default, [`PatternCheck`], is deterministic and uses no model:
//!
//! - text that reads as instructions for an assistant (the phrase table in [`Family`]'s home,
//!   `phrases.rs`, and links that carry credentials);
//! - text hidden from the person with invisible characters ([`HiddenKind`]);
//! - words the tree carries that the person cannot see ([`UnseenWhy`]).
//!
//! A [`ScreenVerdict::Suspicious`] makes the run pause and ask the person; see `FINDINGS.md` for
//! what the check does not catch. A reason holds typed causes and at most one bounded
//! [`Excerpt`] of screen text; the event log keeps the kinds ([`Reason::kind`]) and no text.

mod excerpt;
mod hidden;
mod link;
mod phrases;
mod reader;
mod track;
mod unseen;

pub use excerpt::{EXCERPT_CHARS, Excerpt};
pub use hidden::HiddenKind;
pub use phrases::Family;
pub use reader::{READER_CHARS, ScreenRead, ScreenText};
pub use track::{Asking, ReaderMode, ReaderWait, ScreenTrack, Suspicion};
pub use unseen::{MIN_WORDS, UnseenWhy};

use crate::observation::{Observation, TreeObs};
use a11y_tree::{A11yNode, NodeValue, Role, Sensitivity};
use cua_action::{Size, WindowSpace};
use cua_bus::ScreenKind;
use cua_desktop::Digest32;
use excerpt::{fold, readable};
use porter_core::AppName;
use serde::{Deserialize, Serialize};
use unseen::{Geometry, why_unseen};

/// The most reasons one verdict carries: enough to say what was found, few enough to read.
pub const MAX_REASONS: usize = 6;

/// Texts shorter than this many characters hold no phrase of the table.
const MIN_TEXT_CHARS: usize = 3;

/// One thing that made a screen suspicious.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Reason {
    /// Text that reads as instructions for an assistant.
    Instruction {
        /// The style of attack.
        family: Family,
        /// Where it stands, as screen text.
        excerpt: Excerpt,
    },
    /// Invisible characters. There is nothing visible to quote.
    Hidden {
        /// What they do.
        kind: HiddenKind,
    },
    /// Words the tree carries and the person cannot see.
    Unseen {
        /// Why they cannot be seen.
        why: UnseenWhy,
        /// The start of the words, as screen text.
        excerpt: Excerpt,
    },
    /// The quarantined reader called the screen's text instructions for an assistant. It says
    /// nothing of where, so there is nothing to quote.
    Reader,
}

/// What a reason is caused by: two reasons with one cause are one finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cause {
    Family(Family),
    Hidden(HiddenKind),
    Unseen(UnseenWhy),
    Reader,
}

impl Reason {
    /// The kind the event log keeps.
    pub fn kind(&self) -> ScreenKind {
        match self {
            Reason::Instruction { .. } => ScreenKind::Instruction,
            Reason::Hidden { .. } => ScreenKind::HiddenCharacters,
            Reason::Unseen { .. } => ScreenKind::UnseenText,
            Reason::Reader => ScreenKind::ReaderFlagged,
        }
    }

    /// The bounded piece of screen text, when there is one to show.
    pub fn excerpt(&self) -> Option<&Excerpt> {
        match self {
            Reason::Instruction { excerpt, .. } | Reason::Unseen { excerpt, .. } => Some(excerpt),
            Reason::Hidden { .. } | Reason::Reader => None,
        }
    }

    fn cause(&self) -> Cause {
        match self {
            Reason::Instruction { family, .. } => Cause::Family(*family),
            Reason::Hidden { kind } => Cause::Hidden(*kind),
            Reason::Unseen { why, .. } => Cause::Unseen(*why),
            Reason::Reader => Cause::Reader,
        }
    }
}

/// What a check says about a screen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ScreenVerdict {
    /// Nothing found.
    Clear,
    /// Something found. Never empty, never more than [`MAX_REASONS`], each cause once.
    Suspicious {
        /// What was found, in the order it was found.
        reasons: Vec<Reason>,
    },
}

impl ScreenVerdict {
    /// The verdict for `reasons`: `Clear` when there are none.
    pub fn of(reasons: Vec<Reason>) -> ScreenVerdict {
        if reasons.is_empty() {
            ScreenVerdict::Clear
        } else {
            ScreenVerdict::Suspicious { reasons }
        }
    }

    /// The reasons; none for `Clear`.
    pub fn reasons(&self) -> &[Reason] {
        match self {
            ScreenVerdict::Clear => &[],
            ScreenVerdict::Suspicious { reasons } => reasons,
        }
    }

    /// The kinds of the reasons, each once, in the order found.
    pub fn kinds(&self) -> Vec<ScreenKind> {
        let mut kinds: Vec<ScreenKind> = Vec::new();
        for reason in self.reasons() {
            if !kinds.contains(&reason.kind()) {
                kinds.push(reason.kind());
            }
        }
        kinds
    }
}

/// What a check reads: the text the model is about to be shown, with the geometry that says
/// what the person can see.
#[derive(Debug, Clone, Copy)]
pub struct ScreenView<'a> {
    /// The window's title.
    pub title: &'a str,
    /// The window's logical size.
    pub window: Size<WindowSpace>,
    /// The tree's nodes; none when there is no tree.
    pub nodes: &'a [A11yNode],
}

impl<'a> ScreenView<'a> {
    /// The view of an observation.
    pub fn of(obs: &'a Observation) -> ScreenView<'a> {
        ScreenView {
            title: &obs.window.title.value,
            window: obs.window.size,
            nodes: match &obs.tree {
                TreeObs::Present(snapshot) => &snapshot.nodes,
                TreeObs::Absent(_) => &[],
            },
        }
    }
}

/// Looks at a screen before the model does. The run machine uses [`PatternCheck`]; a stricter
/// deterministic check plugs in behind the same trait, and the quarantined reader adds its own
/// verdict through `CuaModel::classify_screen` (see `RunEffect::ReadScreen`).
pub trait ScreenCheck {
    /// The verdict on `view`. Pure.
    fn check(&self, view: &ScreenView<'_>) -> ScreenVerdict;
}

/// The default check: the phrase table, hidden characters and unseen text. No model.
#[derive(Debug, Clone, Copy, Default)]
pub struct PatternCheck;

/// A node whose content never reaches a model.
fn is_secret(node: &A11yNode) -> bool {
    node.sensitivity == Sensitivity::Secret || node.role == Role::PasswordText
}

/// The text of a node: its name, then its value when that is text.
fn node_text(node: &A11yNode) -> String {
    match &node.value {
        NodeValue::Text(text) => format!("{} {}", node.name.value, text.value),
        NodeValue::None | NodeValue::Range { .. } => node.name.value.clone(),
    }
}

/// What the reasons of one run of the check add up to: each cause once, no more than
/// [`MAX_REASONS`].
#[derive(Default)]
struct Found {
    reasons: Vec<Reason>,
}

impl Found {
    fn add(&mut self, reason: Reason) {
        let cause = reason.cause();
        let known = self.reasons.iter().any(|r| r.cause() == cause);
        if !known && self.reasons.len() < MAX_REASONS {
            self.reasons.push(reason);
        }
    }

    /// The reasons in one text: its hidden characters, then its phrases and links, the latter
    /// also read backwards when a bidi override is there to display them reversed.
    fn read(&mut self, raw: &str) {
        let kinds = hidden::kinds(raw);
        for kind in &kinds {
            self.add(Reason::Hidden { kind: *kind });
        }
        let shown = readable(raw);
        if shown.chars().count() < MIN_TEXT_CHARS {
            return;
        }
        let mut views = vec![shown.clone()];
        if kinds.contains(&HiddenKind::BidiOverride) {
            views.push(shown.chars().rev().collect());
        }
        for view in &views {
            for hit in phrases::scan(&fold(view)) {
                self.add(Reason::Instruction {
                    family: hit.family,
                    excerpt: Excerpt::around(view, hit.at),
                });
            }
            if let Some(excerpt) = link::credential_link(view) {
                self.add(Reason::Instruction {
                    family: Family::CredentialLink,
                    excerpt,
                });
            }
        }
    }

    /// The reason a node's words are unseen, when it has a sentence's worth of them.
    fn read_unseen(&mut self, node: &A11yNode, geometry: Geometry) {
        let Some(why) = why_unseen(node, geometry) else {
            return;
        };
        let shown = readable(&node_text(node));
        if shown.split_whitespace().count() >= MIN_WORDS {
            self.add(Reason::Unseen {
                why,
                excerpt: Excerpt::start_of(&shown),
            });
        }
    }
}

impl ScreenCheck for PatternCheck {
    fn check(&self, view: &ScreenView<'_>) -> ScreenVerdict {
        let geometry = Geometry::of(view.nodes, view.window);
        let mut found = Found::default();
        found.read(view.title);
        for node in view.nodes.iter().filter(|n| !is_secret(n)) {
            found.read(&node.name.value);
            if let NodeValue::Text(text) = &node.value {
                found.read(&text.value);
            }
            found.read_unseen(node, geometry);
        }
        ScreenVerdict::of(found.reasons)
    }
}

/// Which window a finding was about: the app, and a digest of the title (the text itself is not
/// kept). A person's "go on" holds for this window and this kind for the rest of the run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowKey {
    /// The app.
    pub app: AppName,
    /// The blake3 digest of the window's title.
    pub title: Digest32,
}

impl WindowKey {
    /// The key of the window of `app` titled `title`.
    pub fn of(app: &AppName, title: &str) -> WindowKey {
        WindowKey {
            app: app.clone(),
            title: Digest32(*blake3::hash(title.as_bytes()).as_bytes()),
        }
    }
}
