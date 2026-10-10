//! From an action to input operations. Pure.

use crate::caps::SeatCaps;
use crate::caps::TextPath;
use crate::keymap::{Keymap, ShiftLevel};
use crate::op::InputOp;
use crate::op::{Keycode, Press};
use cua_action::{
    Chord, ClickCount, Coord, CuaAction, Modifier, Notches, Point, Repeat, ScrollBy, Size, Target,
    WindowSpace,
};
use cua_desktop::Ms;
use keyboard_types::Key;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The points an action targets, with accessibility nodes already resolved by the caller to the
/// centre of their bounds, clamped inside the window. A resolved point wins over the action's own
/// `Target::Point`; `Target::Centre` is never resolved by the caller: `plan` takes the centre of
/// the window it plans for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ResolvedPoints {
    /// The action targets nothing (a key, a wait).
    None,
    /// One point (click, move, scroll).
    One(Point<WindowSpace>),
    /// A drag.
    Drag {
        /// Where it starts.
        from: Point<WindowSpace>,
        /// Where it ends.
        to: Point<WindowSpace>,
    },
}

/// One action, ready to plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedAction {
    /// The action.
    pub action: CuaAction<WindowSpace>,
    /// Its targets.
    pub at: ResolvedPoints,
    /// The window, for the bounds check.
    pub window: Size<WindowSpace>,
}

/// Why an action cannot become input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum PlanError {
    /// A target outside the window.
    #[error("point outside the window")]
    OutsideWindow(Point<WindowSpace>),
    /// A character no text path can type (`你` with the key-code path only).
    #[error("cannot type {0:?} here")]
    Unmappable(char),
    /// The action is not input (`Finish`, `Ask`, `Zoom`, `Wait`, `Observe`).
    #[error("not an input action")]
    NotAnInput,
    /// A chord key the keymap does not have.
    #[error("unknown key {0}")]
    UnknownKey(String),
    /// A target that names an accessibility node the caller did not resolve to a point.
    #[error("the target was not resolved to a point")]
    Unresolved,
}

/// How long a button is held in a click.
const HOLD: Ms = Ms(40);
/// The pause between the clicks of a double or triple click.
const BETWEEN_CLICKS: Ms = Ms(60);
/// The pause between the strokes of a repeated key.
const BETWEEN_KEYS: Ms = Ms(20);
/// The pointer moves this many times between the press and the release of a drag, the last on
/// the target.
const DRAG_STEPS: u32 = 4;
/// Logical pixels of one scroll notch, for a scroll given as a distance.
const NOTCH_PX: u32 = 40;

/// The centre of a window of `size`: half of each side, rounded down (a window one pixel wide
/// has its centre on pixel 0).
pub fn centre_of(size: Size<WindowSpace>) -> Point<WindowSpace> {
    Point::new(Coord(size.w.0 / 2), Coord(size.h.0 / 2))
}

/// The point `target` lands on. `given` is what the caller resolved (a node's centre, or the
/// point itself); `Centre` is the window's own.
fn resolve(
    target: &Target<WindowSpace>,
    given: Option<Point<WindowSpace>>,
    window: Size<WindowSpace>,
) -> Result<Point<WindowSpace>, PlanError> {
    let point = match target {
        Target::Centre => centre_of(window),
        Target::Point(own) => given.unwrap_or(*own),
        Target::Node(_) => given.ok_or(PlanError::Unresolved)?,
    };
    match point.x.0 < window.w.0 && point.y.0 < window.h.0 {
        true => Ok(point),
        false => Err(PlanError::OutsideWindow(point)),
    }
}

fn one(at: &ResolvedPoints) -> Option<Point<WindowSpace>> {
    match at {
        ResolvedPoints::One(point) => Some(*point),
        ResolvedPoints::None | ResolvedPoints::Drag { .. } => None,
    }
}

fn ends(at: &ResolvedPoints) -> (Option<Point<WindowSpace>>, Option<Point<WindowSpace>>) {
    match at {
        ResolvedPoints::Drag { from, to } => (Some(*from), Some(*to)),
        ResolvedPoints::None | ResolvedPoints::One(_) => (None, None),
    }
}

fn press(button: cua_action::Button, dir: Press) -> InputOp {
    InputOp::Button { button, dir }
}

fn key(code: Keycode, dir: Press) -> InputOp {
    InputOp::Key { key: code, dir }
}

fn modifier_key(modifier: Modifier) -> Key {
    match modifier {
        Modifier::Ctrl => Key::Control,
        Modifier::Alt => Key::Alt,
        Modifier::Shift => Key::Shift,
        Modifier::Super => Key::Meta,
    }
}

fn code_for(keymap: &Keymap, wanted: &Key) -> Result<Keycode, PlanError> {
    keymap
        .code_of(wanted)
        .ok_or_else(|| PlanError::UnknownKey(wanted.to_string()))
}

/// The codes of `mods`, in the modifiers' own order.
fn mod_codes(keymap: &Keymap, mods: &BTreeSet<Modifier>) -> Result<Vec<Keycode>, PlanError> {
    mods.iter()
        .map(|m| code_for(keymap, &modifier_key(*m)))
        .collect()
}

/// `inner` between the modifiers going down (in order) and up (in reverse).
fn bracketed(codes: Vec<Keycode>, inner: Vec<InputOp>) -> Vec<InputOp> {
    let down = codes.iter().map(|c| key(*c, Press::Down));
    let up = codes.iter().rev().map(|c| key(*c, Press::Up));
    down.chain(inner).chain(up).collect()
}

fn clicks(count: ClickCount) -> usize {
    match count {
        ClickCount::One => 1,
        ClickCount::Two => 2,
        ClickCount::Three => 3,
    }
}

fn click_ops(button: cua_action::Button, count: ClickCount) -> Vec<InputOp> {
    (0..clicks(count))
        .flat_map(|n| {
            let gap = (n > 0).then_some(InputOp::Gap(BETWEEN_CLICKS));
            gap.into_iter().chain([
                press(button, Press::Down),
                InputOp::Gap(HOLD),
                press(button, Press::Up),
            ])
        })
        .collect()
}

/// The point `step` of `DRAG_STEPS` of the way from `from` to `to`.
fn along(from: Point<WindowSpace>, to: Point<WindowSpace>, step: u32) -> Point<WindowSpace> {
    let at = |a: u32, b: u32| {
        let (a, b) = (i64::from(a), i64::from(b));
        let value = a + (b - a) * i64::from(step) / i64::from(DRAG_STEPS);
        Coord(u32::try_from(value).unwrap_or(0))
    };
    Point::new(at(from.x.0, to.x.0), at(from.y.0, to.y.0))
}

fn drag_ops(
    from: Point<WindowSpace>,
    to: Point<WindowSpace>,
    button: cua_action::Button,
) -> Vec<InputOp> {
    let moves = (1..=DRAG_STEPS).map(|step| InputOp::MoveTo(along(from, to, step)));
    [
        InputOp::MoveTo(from),
        press(button, Press::Down),
        InputOp::Gap(HOLD),
    ]
    .into_iter()
    .chain(moves)
    .chain([press(button, Press::Up)])
    .collect()
}

fn scroll_ops(
    at: Point<WindowSpace>,
    dir: cua_action::ScrollDir,
    by: ScrollBy<WindowSpace>,
) -> Vec<InputOp> {
    let notches = match by {
        ScrollBy::Notches(n) => n,
        ScrollBy::Distance(len) => {
            let n = len.coord().0.div_ceil(NOTCH_PX).max(1);
            Notches(u16::try_from(n).unwrap_or(u16::MAX))
        }
    };
    vec![InputOp::MoveTo(at), InputOp::Scroll { dir, notches }]
}

fn chord_ops(chord: &Chord, repeat: Repeat, keymap: &Keymap) -> Result<Vec<InputOp>, PlanError> {
    let main = code_for(keymap, &chord.key)?;
    let strokes = (0..repeat.times()).flat_map(|n| {
        let gap = (n > 0).then_some(InputOp::Gap(BETWEEN_KEYS));
        gap.into_iter()
            .chain([key(main, Press::Down), key(main, Press::Up)])
    });
    Ok(bracketed(
        mod_codes(keymap, &chord.mods)?,
        strokes.collect(),
    ))
}

/// One character as strokes: its key, shifted when the keymap says so. A newline and a tab are
/// the Enter and Tab keys.
fn char_ops(c: char, keymap: &Keymap) -> Result<Vec<InputOp>, PlanError> {
    let named = match c {
        '\n' => Some(Key::Enter),
        '\t' => Some(Key::Tab),
        _ => None,
    };
    if let Some(named) = named {
        let code = code_for(keymap, &named)?;
        return Ok(vec![key(code, Press::Down), key(code, Press::Up)]);
    }
    let (code, level) = keymap.stroke_for(c).ok_or(PlanError::Unmappable(c))?;
    let strokes = vec![key(code, Press::Down), key(code, Press::Up)];
    match level {
        ShiftLevel::Plain => Ok(strokes),
        ShiftLevel::Shifted => Ok(bracketed(vec![code_for(keymap, &Key::Shift)?], strokes)),
    }
}

fn type_ops(
    text: &cua_action::TypedText,
    keymap: &Keymap,
    caps: &SeatCaps,
) -> Result<Vec<InputOp>, PlanError> {
    match caps.text.first() {
        Some(TextPath::EiKeymap) => {
            let per_char: Result<Vec<_>, _> =
                text.as_str().chars().map(|c| char_ops(c, keymap)).collect();
            Ok(per_char?.into_iter().flatten().collect())
        }
        Some(TextPath::AgentTextInput | TextPath::EiText | TextPath::VirtualKeyboard) => {
            Ok(vec![InputOp::Text(text.clone())])
        }
        None => Err(PlanError::NotAnInput),
    }
}

/// The operations that perform `action`: a click is move, down, gap, up (repeated for double and
/// triple), a drag is move, down, moves, up, a chord is modifiers down, key down and up (repeated),
/// modifiers up. Refuses a point outside the window. A `Target::Centre` is the centre of
/// `action.window` (`centre_of`); a node is the point the caller resolved in `action.at`. Pure.
pub fn plan(
    action: &PlannedAction,
    keymap: &Keymap,
    caps: &SeatCaps,
) -> Result<Vec<InputOp>, PlanError> {
    let window = action.window;
    match &action.action {
        CuaAction::Click {
            at,
            button,
            count,
            mods,
        } => {
            let point = resolve(at, one(&action.at), window)?;
            let inner = [InputOp::MoveTo(point)];
            let codes = mod_codes(keymap, mods)?;
            let clicking = bracketed(codes, click_ops(*button, *count));
            Ok(inner.into_iter().chain(clicking).collect())
        }
        CuaAction::MoveTo { at } => {
            Ok(vec![InputOp::MoveTo(resolve(at, one(&action.at), window)?)])
        }
        CuaAction::Drag { from, to, button } => {
            let (given_from, given_to) = ends(&action.at);
            let from = resolve(from, given_from, window)?;
            let to = resolve(to, given_to, window)?;
            Ok(drag_ops(from, to, *button))
        }
        CuaAction::Scroll { at, dir, by } => {
            let point = resolve(at, one(&action.at), window)?;
            Ok(scroll_ops(point, *dir, *by))
        }
        CuaAction::Type { text } => type_ops(text, keymap, caps),
        CuaAction::Key { chord, repeat } => chord_ops(chord, *repeat, keymap),
        CuaAction::Wait { .. }
        | CuaAction::Zoom { .. }
        | CuaAction::Observe
        | CuaAction::Finish { .. }
        | CuaAction::Ask { .. } => Err(PlanError::NotAnInput),
        // An action a later stoker adds is not performed until it is planned here.
        _ => Err(PlanError::NotAnInput),
    }
}
