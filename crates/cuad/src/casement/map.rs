//! From what the desktop client reduced to what cuad's seams speak: pure, no connection.

use cua_desktop::{
    DesktopError, DesktopEvent, GlobalCoord, GlobalRect, Pid, ToplevelIdent, ToplevelInfo,
    ToplevelState, WindowTitle, WindowTrust, WorkspaceRef, WorkspaceShown,
};
use desktop_client::geometry::Rect;
use desktop_client::quire::{QuireEvent, QuireWindow, SpacePlace, WindowChange, WindowEvent};
use desktop_client::toplevel::{Toplevel, ToplevelEvent, ToplevelFlag};
use desktop_client::{DesktopEvent as Raised, DesktopState, Protocol, ToplevelKey};
use porter_core::{AppName, Px};
use std::collections::BTreeMap;

/// The identifier of every announced toplevel that has one, by the client's key. The key of a
/// window that closed is gone from the state, so the previous map names it.
pub(super) type Idents = BTreeMap<ToplevelKey, ToplevelIdent>;

/// Every announced toplevel with an identifier, in the client's key order. An error when the
/// compositor offers no toplevel list: the identifiers are what a lease names.
pub(super) fn infos(state: &DesktopState) -> Result<Vec<ToplevelInfo>, DesktopError> {
    if !state.protocols.contains(Protocol::ExtForeignToplevelList) {
        return Err(DesktopError::Protocol(
            "the compositor offers no ext_foreign_toplevel_list_v1".into(),
        ));
    }
    Ok(state
        .toplevels
        .announced()
        .filter_map(|top| info_of(state, top))
        .collect())
}

/// The identifiers of the announced toplevels now.
pub(super) fn idents(state: &DesktopState) -> Idents {
    state
        .toplevels
        .announced()
        .filter_map(|top| Some((top.key, ident_of(top)?)))
        .collect()
}

fn ident_of(top: &Toplevel) -> Option<ToplevelIdent> {
    ToplevelIdent::parse(top.identifier.as_ref()?.as_str()).ok()
}

fn info_of(state: &DesktopState, top: &Toplevel) -> Option<ToplevelInfo> {
    let window = state.quire.window(top.key);
    Some(ToplevelInfo {
        ident: ident_of(top)?,
        app: app_name(&top.app_id),
        title: WindowTitle(top.title.clone()),
        // No protocol here says which process owns a toplevel; 0 is "unknown".
        pid: Pid(0),
        geometry: window
            .and_then(|w| w.geometry)
            .map_or(rect(Rect::new(0, 0, 0, 0)), |g| rect(g.rect)),
        workspace: workspace_of(state, window),
        state: top.flags.iter().filter_map(state_of).collect(),
        trust: trust_of(&top.app_id),
    })
}

fn rect(r: Rect) -> GlobalRect {
    GlobalRect {
        x: GlobalCoord(r.x),
        y: GlobalCoord(r.y),
        w: Px(r.w),
        h: Px(r.h),
    }
}

fn state_of(flag: ToplevelFlag) -> Option<ToplevelState> {
    match flag {
        ToplevelFlag::Maximized => Some(ToplevelState::Maximized),
        ToplevelFlag::Minimized => Some(ToplevelState::Minimized),
        ToplevelFlag::Activated => Some(ToplevelState::Activated),
        _ => None,
    }
}

/// The Space a window is on, and whether that Space is on screen. A window on every Space, or
/// one the compositor has not placed, is shown; a window on a Space with no workspace is not.
fn workspace_of(state: &DesktopState, window: Option<&QuireWindow>) -> WorkspaceRef {
    let id = match window.map(|w| w.place) {
        Some(SpacePlace::On(space)) => state
            .quire
            .space(space)
            .and_then(|s| s.id.as_ref())
            .map(|id| id.as_str().to_owned())
            .unwrap_or_default(),
        _ => {
            return WorkspaceRef {
                id: String::new(),
                shown: WorkspaceShown::Shown,
            };
        }
    };
    let on_screen = state
        .workspaces
        .active()
        .any(|w| !id.is_empty() && w.id.as_deref() == Some(id.as_str()));
    let shown = if on_screen {
        WorkspaceShown::Shown
    } else {
        WorkspaceShown::Hidden
    };
    WorkspaceRef { id, shown }
}

/// The app id as an [`AppName`]. An id that is not reverse-DNS (`konsole`, `Alacritty`) is
/// folded into `app.<id>` with every other character made `_`; an empty one is `app.unknown`.
pub(super) fn app_name(app_id: &str) -> AppName {
    if let Ok(name) = AppName::parse(app_id) {
        return name;
    }
    let folded: String = app_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let folded = if folded.is_empty() {
        "unknown".to_owned()
    } else {
        folded
    };
    let lead = if folded.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        folded
    } else {
        format!("_{folded}")
    };
    AppName::parse(&format!("app.{lead}"))
        .unwrap_or_else(|_| AppName::parse("app.unknown").expect("a valid app name"))
}

/// How far cua may trust a window, from its app id alone (no sandbox information is read
/// here): the shell's own windows are never leasable, other quire apps are ours, everything
/// else is native.
pub(super) fn trust_of(app_id: &str) -> WindowTrust {
    const SHELL: [&str; 3] = ["org.quire.Shell", "org.quire.Sill", "org.quire.Desktop"];
    if SHELL.iter().any(|shell| app_id.starts_with(shell)) {
        WindowTrust::Shell
    } else if app_id.starts_with("org.quire.") {
        WindowTrust::Quire
    } else {
        WindowTrust::Native
    }
}

/// What `raised` tells cuad: a window that closed (named from `before`, since the state has
/// forgotten it) and one that moved or resized (named from `now`).
pub(super) fn events(raised: &[Raised], before: &Idents, now: &Idents) -> Vec<DesktopEvent> {
    raised
        .iter()
        .filter_map(|event| match event {
            Raised::Toplevel(ToplevelEvent::Closed(key)) => {
                before.get(key).cloned().map(DesktopEvent::WindowClosed)
            }
            Raised::Quire(QuireEvent::Window(WindowEvent::Changed {
                key,
                change: WindowChange::Geometry(g),
            })) => now
                .get(key)
                .map(|ident| DesktopEvent::GeometryChanged(ident.clone(), rect(g.rect))),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use desktop_client::quire::{SpaceChange, SpaceEvent, WindowGeometry};
    use desktop_client::toplevel::{ToplevelChange, ToplevelFlags};
    use desktop_client::workspace::{WorkspaceChange, WorkspaceEvent, WorkspaceFlag};
    use desktop_client::{
        ProtocolSet, SpaceId, SpaceKey, ToplevelIdentifier, WorkspaceKey, reduce,
    };
    use std::collections::BTreeSet;

    fn tk(n: u64) -> ToplevelKey {
        ToplevelKey::new(n)
    }

    fn run(events: Vec<Raised>) -> DesktopState {
        let start = DesktopState::new(ProtocolSet::empty().with(Protocol::ExtForeignToplevelList));
        events.into_iter().fold(start, reduce)
    }

    fn change(key: ToplevelKey, change: ToplevelChange) -> Raised {
        Raised::Toplevel(ToplevelEvent::Changed { key, change })
    }

    /// An announced toplevel with its identifier, app id and title.
    fn window(n: u64, ident: &str, app: &str, title: &str) -> Vec<Raised> {
        vec![
            Raised::Toplevel(ToplevelEvent::Opened(tk(n))),
            change(
                tk(n),
                ToplevelChange::Identifier(ToplevelIdentifier::new(ident)),
            ),
            change(tk(n), ToplevelChange::AppId(app.into())),
            change(tk(n), ToplevelChange::Title(title.into())),
            Raised::Toplevel(ToplevelEvent::Done(tk(n))),
        ]
    }

    fn quire(event: WindowEvent) -> Raised {
        Raised::Quire(QuireEvent::Window(event))
    }

    fn placed(n: u64, place: SpacePlace, rect: Rect) -> Vec<Raised> {
        vec![
            quire(WindowEvent::Opened(tk(n))),
            quire(WindowEvent::Changed {
                key: tk(n),
                change: WindowChange::Place(place),
            }),
            quire(WindowEvent::Changed {
                key: tk(n),
                change: WindowChange::Geometry(WindowGeometry::new(rect, 120)),
            }),
            quire(WindowEvent::Done(tk(n))),
        ]
    }

    #[test]
    fn app_ids_become_app_names() {
        let table = [
            ("org.gnome.Calculator", "org.gnome.Calculator"),
            ("konsole", "app.konsole"),
            ("Alacritty", "app.Alacritty"),
            ("", "app.unknown"),
            ("9lives", "app._9lives"),
            ("my app!", "app.my_app_"),
        ];
        for (id, want) in table {
            assert_eq!(app_name(id).as_str(), want, "{id:?}");
        }
    }

    #[test]
    fn trust_follows_the_app_id() {
        let table = [
            ("org.quire.Shell", WindowTrust::Shell),
            ("org.quire.Sill.Bar", WindowTrust::Shell),
            ("org.quire.Photos", WindowTrust::Quire),
            ("org.gnome.Calculator", WindowTrust::Native),
            ("", WindowTrust::Native),
        ];
        for (id, want) in table {
            assert_eq!(trust_of(id), want, "{id:?}");
        }
    }

    #[test]
    fn no_toplevel_list_is_a_protocol_error() {
        let state = DesktopState::default();
        assert!(matches!(infos(&state), Err(DesktopError::Protocol(_))));
    }

    #[test]
    fn a_toplevel_becomes_its_info_with_the_quire_geometry_and_flags() {
        let mut script = window(1, "w1", "org.gnome.Calculator", "Calculator");
        script.extend(placed(
            1,
            SpacePlace::AllSpaces,
            Rect::new(-10, 20, 400, 300),
        ));
        script.push(change(
            tk(1),
            ToplevelChange::Flags(ToplevelFlags::of([
                ToplevelFlag::Activated,
                ToplevelFlag::Fullscreen,
            ])),
        ));
        // Not announced yet: not listed.
        script.push(Raised::Toplevel(ToplevelEvent::Opened(tk(2))));
        script.extend(window(3, "w3", "konsole", "shell"));
        let all = infos(&run(script)).unwrap();
        let idents: Vec<_> = all.iter().map(|w| w.ident.as_str()).collect();
        assert_eq!(idents, ["w1", "w3"]);
        let first = &all[0];
        assert_eq!(first.app.as_str(), "org.gnome.Calculator");
        assert_eq!(first.title, WindowTitle("Calculator".into()));
        assert_eq!(first.pid, Pid(0));
        assert_eq!(first.trust, WindowTrust::Native);
        assert_eq!(first.state, BTreeSet::from([ToplevelState::Activated]));
        assert_eq!(first.geometry, rect(Rect::new(-10, 20, 400, 300)));
        assert_eq!(first.workspace.shown, WorkspaceShown::Shown);
        assert_eq!(all[1].app.as_str(), "app.konsole");
        assert_eq!(all[1].geometry, rect(Rect::new(0, 0, 0, 0)));
    }

    #[test]
    fn a_window_is_shown_when_its_space_is_the_active_workspace() {
        let space = |n: u64, id: &str| {
            let key = SpaceKey::new(n);
            vec![
                Raised::Quire(QuireEvent::Space(SpaceEvent::Opened(key))),
                Raised::Quire(QuireEvent::Space(SpaceEvent::Changed {
                    key,
                    change: SpaceChange::Id(SpaceId::new(id)),
                })),
                Raised::Quire(QuireEvent::Space(SpaceEvent::Done(key))),
            ]
        };
        let workspace = |n: u64, id: &str, flags: BTreeSet<WorkspaceFlag>| {
            let key = WorkspaceKey::new(n);
            vec![
                Raised::Workspace(WorkspaceEvent::Opened(key)),
                Raised::Workspace(WorkspaceEvent::Changed {
                    key,
                    change: WorkspaceChange::Id(id.into()),
                }),
                Raised::Workspace(WorkspaceEvent::Changed {
                    key,
                    change: WorkspaceChange::Flags(flags),
                }),
            ]
        };
        let mut script = space(1, "s-main");
        script.extend(space(2, "s-other"));
        script.extend(workspace(
            1,
            "s-main",
            BTreeSet::from([WorkspaceFlag::Active]),
        ));
        script.extend(workspace(2, "s-other", BTreeSet::new()));
        script.push(Raised::Workspace(WorkspaceEvent::Done));
        script.extend(window(1, "w1", "a.b", "one"));
        script.extend(placed(
            1,
            SpacePlace::On(SpaceKey::new(1)),
            Rect::new(0, 0, 1, 1),
        ));
        script.extend(window(2, "w2", "a.b", "two"));
        script.extend(placed(
            2,
            SpacePlace::On(SpaceKey::new(2)),
            Rect::new(0, 0, 1, 1),
        ));
        let all = infos(&run(script)).unwrap();
        assert_eq!(all[0].workspace.id, "s-main");
        assert_eq!(all[0].workspace.shown, WorkspaceShown::Shown);
        assert_eq!(all[1].workspace.id, "s-other");
        assert_eq!(all[1].workspace.shown, WorkspaceShown::Hidden);
    }

    #[test]
    fn closing_and_moving_become_desktop_events() {
        let mut script = window(1, "w1", "a.b", "one");
        script.extend(window(2, "w2", "a.b", "two"));
        let before = idents(&run(script));
        let moved = WindowGeometry::new(Rect::new(5, 6, 7, 8), 120);
        let raised = vec![
            quire(WindowEvent::Changed {
                key: tk(1),
                change: WindowChange::Geometry(moved),
            }),
            Raised::Toplevel(ToplevelEvent::Closed(tk(2))),
            // A window that was never known says nothing.
            Raised::Toplevel(ToplevelEvent::Closed(tk(9))),
        ];
        let now = BTreeMap::from([(tk(1), before[&tk(1)].clone())]);
        assert_eq!(
            events(&raised, &before, &now),
            vec![
                DesktopEvent::GeometryChanged(
                    ToplevelIdent::parse("w1").unwrap(),
                    rect(Rect::new(5, 6, 7, 8))
                ),
                DesktopEvent::WindowClosed(ToplevelIdent::parse("w2").unwrap()),
            ]
        );
    }
}
