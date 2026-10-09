//! The data a fake surface is made of: screens, the items on them, and what a click does. A
//! surface is a small deterministic application, written as a TOML file under `eval/surfaces/`.

use serde::Deserialize;

/// A whole fake application.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SurfaceDef {
    /// The app id of its window; it picks the window class (a terminal, a mail client).
    pub app: String,
    /// The window's size in pixels.
    pub size: [u32; 2],
    /// Its screens.
    #[serde(rename = "screen")]
    pub screens: Vec<ScreenDef>,
}

/// One screen of the application. Its title is the window's title while it shows.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ScreenDef {
    /// Its id, the target of a `goto`.
    pub id: String,
    /// The window title while this screen shows.
    pub title: String,
    /// What Enter does when no field has the focus.
    #[serde(default)]
    pub enter: Vec<Act>,
    /// What Escape does.
    #[serde(default)]
    pub escape: Vec<Act>,
    /// The id of the field that has the focus when the screen shows (a terminal's prompt, the
    /// first line of a form); none when nothing does.
    #[serde(default)]
    pub focus: Option<String>,
    /// The scrolling pane, when the screen has one.
    pub pane: Option<PaneDef>,
    /// Its items, back to front.
    #[serde(default, rename = "item")]
    pub items: Vec<ItemDef>,
}

/// The part of a screen that scrolls: items placed `pane` move with it and are clipped to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct PaneDef {
    /// The top edge of the viewport.
    pub top: u32,
    /// The bottom edge of the viewport.
    pub bottom: u32,
    /// The height of everything in the pane.
    pub content: u32,
}

/// What an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Text.
    Label,
    /// A filled button.
    Button,
    /// A list row: an outlined line of text that can be clicked.
    Row,
    /// A text field; typing goes to the one with the focus.
    Field,
    /// A box that toggles between `on` and `off`.
    Check,
}

/// Whether an item stays where it is or moves with the pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Place {
    /// Fixed in the window.
    #[default]
    Fixed,
    /// Scrolls with the pane.
    Pane,
    /// Part of the context menu: `rect` is an offset from the point where the menu was opened,
    /// and the item shows only while the menu is open.
    Menu,
}

/// How a field shows what was typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Echo {
    /// As typed.
    #[default]
    Plain,
    /// As dots.
    Masked,
    /// On the window itself after its text, with no box, as a terminal's prompt line.
    Inline,
}

/// One item of a screen.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ItemDef {
    /// What it is.
    pub kind: Kind,
    /// Its id: the name of the field it holds or the variable a check toggles.
    #[serde(default)]
    pub id: String,
    /// Left, top, width and height; a label's width and height are ignored.
    pub rect: [u32; 4],
    /// Its text. `{name}` is replaced by the variable or field of that name.
    #[serde(default)]
    pub text: String,
    /// What a click does.
    #[serde(default)]
    pub click: Vec<Act>,
    /// What Enter does while this field has the focus.
    #[serde(default)]
    pub enter: Vec<Act>,
    /// What a right click does. A right click on an item with acts here runs them and opens the
    /// context menu (the `menu` items) at the pointer.
    #[serde(default)]
    pub context: Vec<Act>,
    /// Whether it scrolls.
    #[serde(default)]
    pub place: Place,
    /// How a field shows its text.
    #[serde(default)]
    pub echo: Echo,
    /// The item shows only while all of these hold.
    #[serde(default)]
    pub show: Vec<Cond>,
}

/// What a click or a key does to the application.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Act {
    /// Show this screen.
    Goto(String),
    /// Set a variable: `[name, value]`; `{name}` in the value is filled in.
    Set([String; 2]),
    /// Copy a field into a variable: `[field, variable]`.
    Copy([String; 2]),
    /// Empty a field.
    Clear(String),
    /// Do one list or the other, by a condition.
    When(When),
}

/// A conditional act.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct When {
    /// The condition.
    #[serde(rename = "if")]
    pub cond: Cond,
    /// Done when it holds.
    pub then: Vec<Act>,
    /// Done when it does not.
    #[serde(default)]
    pub otherwise: Vec<Act>,
}

/// A statement about the application's state: a task's success, a task's forbidden outcome, and
/// the condition of a `when`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cond {
    /// This screen shows.
    Screen(String),
    /// A field holds exactly this: `[field, value]`.
    Field([String; 2]),
    /// A field or a variable contains this: `[name, text]`.
    Has([String; 2]),
    /// A variable is exactly this: `[name, value]`.
    Var([String; 2]),
    /// A field or a variable contains this, in any case: `[name, text]`.
    Like([String; 2]),
    /// The condition does not hold.
    Not(Box<Cond>),
}
