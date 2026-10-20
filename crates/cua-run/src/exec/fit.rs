//! Whether input may go to the leased window right now.
//!
//! The aim of an action was made against an observation. Before it is injected the window is read
//! again: input only goes to a window that is still where and what it was aimed at, visible, and
//! (on a shared seat, where keys land in whichever window has the focus) focused for an action
//! that sends keys. A pointer action needs no focus: a click is how a window gets it.

use super::observe::visibility_of;
use crate::machine::input::Moved;
use crate::observation::{Focus, Visibility};
use cua_action::{Coord, CuaAction, Size, WindowSpace};
use cua_desktop::{GlobalRect, Pid, SeatIsolation, ToplevelInfo, ToplevelState};
use porter_core::AppName;

/// The window as the last observation saw it: what an action was aimed at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Seen {
    app: AppName,
    pid: Pid,
    geometry: GlobalRect,
    focus: Focus,
}

fn focus_of(window: &ToplevelInfo) -> Focus {
    if window.state.contains(&ToplevelState::Activated) {
        Focus::Focused
    } else {
        Focus::Unfocused
    }
}

impl Seen {
    pub(super) fn of(window: &ToplevelInfo) -> Self {
        Self {
            app: window.app.clone(),
            pid: window.pid,
            geometry: window.geometry,
            focus: focus_of(window),
        }
    }

    /// What about the window is not as it was seen, if anything.
    pub(super) fn moved_by(&self, now: &ToplevelInfo) -> Option<Moved> {
        if self.app != now.app || self.pid != now.pid {
            Some(Moved::Identity)
        } else if self.geometry != now.geometry {
            Some(Moved::Geometry)
        } else if self.focus != focus_of(now) {
            Some(Moved::Focus)
        } else {
            None
        }
    }
}

/// What the window is fit for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fit {
    /// Input may go.
    Ready,
    /// Minimized or on a workspace that is not shown: it takes no input.
    NotVisible,
    /// Another size than the one the action was aimed at.
    Resized,
    /// Another window has the focus, and the seat is the person's.
    NotFocused,
}

/// What an action sends: keys go to the focused window, the pointer to the one under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Takes {
    /// Keys or text.
    Keys,
    /// The pointer, or nothing at all.
    Pointer,
}

/// What `action` sends. A click with modifiers holds keys down, and keys go to the focused
/// window. `CuaAction` is non-exhaustive: an action this does not name yet is taken as keys, the
/// stricter check, until it is decided here.
pub(super) fn takes(action: &CuaAction<WindowSpace>) -> Takes {
    match action {
        CuaAction::Type { .. } | CuaAction::Key { .. } => Takes::Keys,
        CuaAction::Click { mods, .. } if !mods.is_empty() => Takes::Keys,
        CuaAction::Click { .. }
        | CuaAction::MoveTo { .. }
        | CuaAction::Drag { .. }
        | CuaAction::Scroll { .. }
        | CuaAction::Wait { .. }
        | CuaAction::Zoom { .. }
        | CuaAction::Observe
        | CuaAction::Finish { .. }
        | CuaAction::Ask { .. } => Takes::Pointer,
        _ => Takes::Keys,
    }
}

/// The window as the desktop reports it now, against the size the action was aimed at.
pub(super) fn fit_of(
    window: &ToplevelInfo,
    aimed: Size<WindowSpace>,
    isolation: SeatIsolation,
    takes: Takes,
) -> Fit {
    let size = Size::new(Coord(window.geometry.w.0), Coord(window.geometry.h.0));
    if visibility_of(window) != Visibility::Shown {
        Fit::NotVisible
    } else if size != aimed {
        Fit::Resized
    } else if isolation == SeatIsolation::SharedSeat
        && takes == Takes::Keys
        && !window.state.contains(&ToplevelState::Activated)
    {
        Fit::NotFocused
    } else {
        Fit::Ready
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_desktop::{
        GlobalCoord, GlobalRect, Pid, ToplevelIdent, WindowTitle, WorkspaceRef, WorkspaceShown,
    };
    use porter_core::{AppName, Px};
    use std::collections::BTreeSet;

    fn window(state: &[ToplevelState]) -> ToplevelInfo {
        ToplevelInfo {
            ident: ToplevelIdent::parse("w1").expect("an identifier"),
            app: AppName::parse("org.gnome.Calculator").expect("an app"),
            title: WindowTitle("Calculator".into()),
            pid: Pid(1),
            geometry: GlobalRect {
                x: GlobalCoord(0),
                y: GlobalCoord(0),
                w: Px(100),
                h: Px(80),
            },
            workspace: WorkspaceRef {
                id: String::new(),
                shown: WorkspaceShown::Shown,
            },
            state: state.iter().copied().collect::<BTreeSet<_>>(),
            trust: cua_desktop::WindowTrust::Native,
        }
    }

    #[test]
    fn a_window_that_is_not_as_it_was_seen_names_what_moved() {
        let seen = Seen::of(&window(&[ToplevelState::Activated]));
        let mut other_pid = window(&[ToplevelState::Activated]);
        other_pid.pid = Pid(2);
        let mut moved = window(&[ToplevelState::Activated]);
        moved.geometry.x = GlobalCoord(5);
        let table = [
            (window(&[ToplevelState::Activated]), None),
            (other_pid, Some(Moved::Identity)),
            (moved, Some(Moved::Geometry)),
            (window(&[]), Some(Moved::Focus)),
        ];
        for (now, want) in table {
            assert_eq!(seen.moved_by(&now), want, "{now:?}");
        }
    }

    #[test]
    fn keys_and_a_click_with_modifiers_take_the_keyboard() {
        use cua_action::{Button, ClickCount, Modifier, Point, Target};
        let click = |mods: &[Modifier]| CuaAction::Click {
            at: Target::Point(Point::new(Coord(1), Coord(1))),
            button: Button::Left,
            count: ClickCount::One,
            mods: mods.iter().copied().collect(),
        };
        let table = [
            ("a plain click", click(&[]), Takes::Pointer),
            ("a click with Ctrl", click(&[Modifier::Ctrl]), Takes::Keys),
            ("a look", CuaAction::Observe, Takes::Pointer),
        ];
        for (name, action, want) in table {
            assert_eq!(takes(&action), want, "{name}");
        }
    }

    #[test]
    fn input_needs_the_aimed_size_visibility_and_on_a_shared_seat_the_focus() {
        let aimed = Size::new(Coord(100), Coord(80));
        let wrong = Size::new(Coord(100), Coord(81));
        let focused = window(&[ToplevelState::Activated]);
        let unfocused = window(&[]);
        let minimized = window(&[ToplevelState::Activated, ToplevelState::Minimized]);
        let shared = SeatIsolation::SharedSeat;
        let own = SeatIsolation::AgentSeat;
        let (keys, pointer) = (Takes::Keys, Takes::Pointer);
        let table = [
            (&focused, aimed, shared, keys, Fit::Ready),
            (&focused, wrong, shared, pointer, Fit::Resized),
            (&unfocused, aimed, shared, keys, Fit::NotFocused),
            (&unfocused, aimed, shared, pointer, Fit::Ready),
            (&unfocused, aimed, own, keys, Fit::Ready),
            (&minimized, aimed, own, pointer, Fit::NotVisible),
        ];
        for (window, size, seat, takes, want) in table {
            assert_eq!(
                fit_of(window, size, seat, takes),
                want,
                "{window:?} {seat:?} {takes:?}"
            );
        }
    }
}
