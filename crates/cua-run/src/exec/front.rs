//! The window a run looks at: the leased window, or the dialog it opened.
//!
//! A window that opened a dialog (a file chooser, a question) waits on it: what the person sees,
//! and where the next click has to go, is the dialog. The compositor lets a lease's input reach
//! the windows its own window opened, so the run follows the dialog: it looks at it, aims at it
//! and acts on it, and is back on the window once the dialog closes. Only a shown window of the
//! same app is followed; another app's window stays outside the lease.

use super::observe::visibility_of;
use crate::observation::Visibility;
use cua_desktop::{ToplevelIdent, ToplevelInfo};

/// The window in front of `leased` among `all`: the last listed shown window of the same app
/// that `leased` opened, then the one that opened in turn, and so on; `None` when `leased` is
/// not listed.
pub(super) fn front_of<'a>(
    leased: &ToplevelIdent,
    all: &'a [ToplevelInfo],
) -> Option<&'a ToplevelInfo> {
    let mut front = all.iter().find(|w| &w.ident == leased)?;
    // One window further down the chain each time: no chain is longer than the list.
    for _ in 0..all.len() {
        let opened = all.iter().rev().find(|w| {
            w.parent.as_ref() == Some(&front.ident)
                && w.app == front.app
                && visibility_of(w) == Visibility::Shown
        });
        match opened {
            Some(dialog) => front = dialog,
            None => break,
        }
    }
    Some(front)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cua_desktop::{
        GlobalCoord, GlobalRect, Pid, ToplevelState, WindowTitle, WindowTrust, WorkspaceRef,
        WorkspaceShown,
    };
    use porter_core::{AppName, Px};
    use std::collections::BTreeSet;

    fn window(ident: &str, app: &str, parent: Option<&str>) -> ToplevelInfo {
        ToplevelInfo {
            ident: ToplevelIdent::parse(ident).unwrap(),
            app: AppName::parse(app).unwrap(),
            title: WindowTitle(ident.into()),
            pid: Pid(7),
            geometry: GlobalRect {
                x: GlobalCoord(0),
                y: GlobalCoord(0),
                w: Px(400),
                h: Px(300),
            },
            workspace: WorkspaceRef {
                id: "ws-1".into(),
                shown: WorkspaceShown::Shown,
            },
            state: BTreeSet::new(),
            trust: WindowTrust::Native,
            parent: parent.map(|p| ToplevelIdent::parse(p).unwrap()),
        }
    }

    fn minimized(mut w: ToplevelInfo) -> ToplevelInfo {
        w.state.insert(ToplevelState::Minimized);
        w
    }

    #[test]
    fn the_run_looks_at_the_dialog_its_window_opened_and_at_nothing_else() {
        const APP: &str = "org.example.Draw";
        let leased = ToplevelIdent::parse("w1").unwrap();
        let front =
            |all: &[ToplevelInfo]| front_of(&leased, all).map(|w| w.ident.as_str().to_owned());
        let main = window("w1", APP, None);
        let cases: [(&str, Vec<ToplevelInfo>, Option<&str>); 7] = [
            ("alone", vec![main.clone()], Some("w1")),
            ("not listed", vec![window("w2", APP, None)], None),
            (
                "a dialog",
                vec![main.clone(), window("d1", APP, Some("w1"))],
                Some("d1"),
            ),
            (
                "a dialog's dialog",
                vec![
                    main.clone(),
                    window("d1", APP, Some("w1")),
                    window("d2", APP, Some("d1")),
                ],
                Some("d2"),
            ),
            (
                "another window of the app",
                vec![main.clone(), window("w2", APP, None)],
                Some("w1"),
            ),
            (
                "another app's window over it",
                vec![main.clone(), window("p1", "org.example.Portal", Some("w1"))],
                Some("w1"),
            ),
            (
                "a minimized dialog",
                vec![main.clone(), minimized(window("d1", APP, Some("w1")))],
                Some("w1"),
            ),
        ];
        for (case, all, want) in cases {
            assert_eq!(front(&all).as_deref(), want, "{case}");
        }
    }
}
