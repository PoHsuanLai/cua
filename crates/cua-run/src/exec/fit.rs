//! Whether input may go to the leased window right now.
//!
//! The aim of an action was made against an observation. Before it is injected the window is read
//! again: input only goes to a window that is still where and what it was aimed at, visible, and
//! (on a shared seat, where keys land in whichever window has the focus) focused.

use super::observe::visibility_of;
use crate::observation::Visibility;
use cua_action::{Coord, Size, WindowSpace};
use cua_desktop::{SeatIsolation, ToplevelInfo, ToplevelState};

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

/// The window as the desktop reports it now, against the size the action was aimed at.
pub(super) fn fit_of(
    window: &ToplevelInfo,
    aimed: Size<WindowSpace>,
    isolation: SeatIsolation,
) -> Fit {
    let size = Size::new(Coord(window.geometry.w.0), Coord(window.geometry.h.0));
    if visibility_of(window) != Visibility::Shown {
        Fit::NotVisible
    } else if size != aimed {
        Fit::Resized
    } else if isolation == SeatIsolation::SharedSeat
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
    fn input_needs_the_aimed_size_visibility_and_on_a_shared_seat_the_focus() {
        let aimed = Size::new(Coord(100), Coord(80));
        let wrong = Size::new(Coord(100), Coord(81));
        let focused = window(&[ToplevelState::Activated]);
        let unfocused = window(&[]);
        let minimized = window(&[ToplevelState::Activated, ToplevelState::Minimized]);
        let shared = SeatIsolation::SharedSeat;
        let own = SeatIsolation::AgentSeat;
        let table = [
            (&focused, aimed, shared, Fit::Ready),
            (&focused, wrong, shared, Fit::Resized),
            (&unfocused, aimed, shared, Fit::NotFocused),
            (&unfocused, aimed, own, Fit::Ready),
            (&minimized, aimed, own, Fit::NotVisible),
        ];
        for (window, size, seat, want) in table {
            assert_eq!(fit_of(window, size, seat), want, "{window:?} {seat:?}");
        }
    }
}
