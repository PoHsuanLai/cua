//! The one surface of a run, shared by the fake seat that acts on it, the fake capture that draws
//! it and the fake desktop that names its window. It also keeps watch: every forbidden outcome
//! of the task is checked after each input, so a violation that is undone a step later is still
//! seen.

use crate::keys::{Decoded, decode};
use crate::surface::{Cond, Surface, render, snapshot};
use a11y_tree::A11ySnapshot;
use chordkit::{Action, Context, Key, KeyInput, Keymap, Modifier, Modifiers};
use cua_action::Button;
use cua_desktop::Stamp;
use maskcap::PixelBuf;
use prov::Label;
use seat_input::{InputOp, Press};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard};

#[derive(Debug)]
struct State {
    surface: Surface,
    pointer: [u32; 2],
    down: Option<[u32; 2]>,
    down_right: Option<[u32; 2]>,
    held: Modifiers,
    generation: u64,
    reported: u64,
    violated: BTreeSet<usize>,
    exposure: Exposure,
}

/// Whether the screen ever showed what the task plants on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exposure {
    /// Never (or the task plants nothing).
    #[default]
    Unseen,
    /// At some point.
    Seen,
}

/// A surface being acted on, and the outcomes that must not happen on it.
#[derive(Debug)]
pub struct Stage {
    state: Mutex<State>,
    forbid: Vec<Cond>,
    exposure: Vec<Cond>,
    keymap: Keymap,
}

/// What a window looks like to the desktop right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowNow {
    /// The app id.
    pub app: String,
    /// The title.
    pub title: String,
    /// The size in pixels.
    pub size: [u32; 2],
}

impl Stage {
    /// A stage over `surface`, watching for `forbid` and for the screen that shows `exposure`.
    /// A shortcut works on it only as `keymap` (the platform's) binds it: the chord of another
    /// platform does nothing.
    pub fn new(surface: Surface, forbid: Vec<Cond>, exposure: Vec<Cond>, keymap: Keymap) -> Stage {
        let stage = Stage {
            state: Mutex::new(State {
                surface,
                pointer: [0, 0],
                down: None,
                down_right: None,
                held: Modifiers::NONE,
                generation: 0,
                reported: 0,
                violated: BTreeSet::new(),
                exposure: Exposure::Unseen,
            }),
            forbid,
            exposure,
            keymap,
        };
        stage.watch(&mut stage.lock());
        stage
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().expect("stage lock")
    }

    fn watch(&self, state: &mut State) {
        let planted = !self.exposure.is_empty();
        if planted && self.exposure.iter().all(|c| state.surface.holds(c)) {
            state.exposure = Exposure::Seen;
        }
        for (n, cond) in self.forbid.iter().enumerate() {
            if state.surface.holds(cond) {
                state.violated.insert(n);
            }
        }
    }

    /// The window as the desktop lists it.
    pub fn window(&self) -> WindowNow {
        let state = self.lock();
        WindowNow {
            app: state.surface.app().to_owned(),
            title: state.surface.title(),
            size: state.surface.size(),
        }
    }

    /// The window drawn.
    pub fn draw(&self) -> PixelBuf {
        render(&self.lock().surface)
    }

    /// The window's accessibility tree now, taken at `taken`, its names labelled `label`.
    pub fn tree(&self, taken: Stamp, label: &Label) -> A11ySnapshot {
        snapshot(&self.lock().surface, taken, label)
    }

    /// Whether the screen changed since the last time this was asked.
    pub fn changed(&self) -> ScreenChange {
        let mut state = self.lock();
        let change = if state.generation == state.reported {
            ScreenChange::Same
        } else {
            ScreenChange::Changed(state.surface.size())
        };
        state.reported = state.generation;
        change
    }

    /// Whether `cond` holds on the surface now.
    pub fn holds(&self, cond: &Cond) -> bool {
        self.lock().surface.holds(cond)
    }

    /// The id of the screen that shows.
    pub fn screen_id(&self) -> String {
        self.lock().surface.screen_id().to_owned()
    }

    /// The middle of the item named `id`, when it shows.
    pub fn centre_of(&self, id: &str) -> Option<[u32; 2]> {
        self.lock().surface.centre_of(id)
    }

    /// Whether the planted text was ever on the screen.
    pub fn exposure(&self) -> Exposure {
        self.lock().exposure
    }

    /// The indexes of the forbidden outcomes that happened at any time, in order.
    pub fn violated(&self) -> Vec<usize> {
        self.lock().violated.iter().copied().collect()
    }

    /// Input arrives: the operations act on the surface in order.
    pub fn apply(&self, ops: &[InputOp]) {
        let mut state = self.lock();
        for op in ops {
            act(&mut state, &self.keymap, op);
            self.watch(&mut state);
        }
        state.generation += 1;
    }
}

/// Whether the screen differs from the last report.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenChange {
    /// It does not.
    Same,
    /// It does; the window's size.
    Changed([u32; 2]),
}

fn act(state: &mut State, keymap: &Keymap, op: &InputOp) {
    match op {
        InputOp::MoveTo(p) => state.pointer = [p.x.0, p.y.0],
        InputOp::Button {
            button: Button::Left,
            dir: Press::Down,
        } => state.down = Some(state.pointer),
        InputOp::Button {
            button: Button::Left,
            dir: Press::Up,
        } => {
            if state.down.take() == Some(state.pointer) {
                let [x, y] = state.pointer;
                state.surface.click(x, y);
            }
        }
        InputOp::Button {
            button: Button::Right,
            dir: Press::Down,
        } => state.down_right = Some(state.pointer),
        InputOp::Button {
            button: Button::Right,
            dir: Press::Up,
        } => {
            if state.down_right.take() == Some(state.pointer) {
                let [x, y] = state.pointer;
                state.surface.context_click(x, y);
            }
        }
        InputOp::Button { .. } => {}
        InputOp::Key { key, dir } => key_op(state, keymap, decode(*key), *dir),
        InputOp::Text(text) => state.surface.type_text(text.as_str()),
        InputOp::Scroll { dir, notches } => state.surface.scroll(*dir, notches.0),
        InputOp::Gap(_) => {}
    }
}

fn key_op(state: &mut State, keymap: &Keymap, key: Decoded, dir: Press) {
    // Shift alone only changes what a character is; with another modifier it is part of a chord.
    let chorded = !state.held.without(Modifier::Shift).is_empty();
    match (key, dir) {
        (Decoded::Modifier(m), Press::Down) => state.held = state.held.with(m),
        (Decoded::Modifier(m), Press::Up) => state.held = state.held.without(m),
        (_, Press::Up) | (Decoded::Other, _) => {}
        (Decoded::Char(c), Press::Down) if chorded => shortcut(state, keymap, c),
        (_, Press::Down) if chorded => {}
        (Decoded::Char(c), Press::Down) => state.surface.type_text(&c.to_string()),
        (Decoded::Stroke(stroke), Press::Down) => state.surface.press(stroke),
    }
}

/// A character key pressed with the modifiers held: the surface does what its screen says for the
/// standard action the platform's keymap binds that chord to, and nothing when it binds none.
fn shortcut(state: &mut State, keymap: &Keymap, c: char) {
    let context = match state.surface.focused() {
        Some(_) => Context::TextEntry,
        None => Context::Normal,
    };
    let input = KeyInput::new(state.held, Key::character(c));
    if let Some(Action::Standard(action)) = keymap.resolve(&input, context) {
        state.surface.shortcut(&action.slug());
    }
}
