//! From what the handlers recorded to [`ToplevelInfo`].

use super::state::{Toplevel, WlState};
use crate::{
    DesktopError, GlobalCoord, GlobalRect, Pid, ToplevelIdent, ToplevelInfo, WindowTitle,
    WindowTrust, WorkspaceRef, WorkspaceShown,
};
use porter_core::{AppName, Px};

/// Every announced toplevel with an identifier, in the order the compositor made them.
pub(crate) fn infos(state: &WlState) -> Result<Vec<ToplevelInfo>, DesktopError> {
    if !state.has_list() {
        return Err(DesktopError::Protocol(
            "the compositor offers no ext_foreign_toplevel_list_v1".into(),
        ));
    }
    Ok(state
        .toplevels
        .values()
        .filter(|top| top.announced)
        .filter_map(|top| info_of(state, top))
        .collect())
}

fn info_of(state: &WlState, top: &Toplevel) -> Option<ToplevelInfo> {
    let ident = ToplevelIdent::parse(top.ident.as_deref()?).ok()?;
    let workspace = top
        .workspaces
        .iter()
        .find_map(|key| state.workspaces.get(key))
        .map_or(
            WorkspaceRef {
                id: String::new(),
                shown: WorkspaceShown::Shown,
            },
            |ws| WorkspaceRef {
                id: ws.id.clone(),
                shown: if ws.active {
                    WorkspaceShown::Shown
                } else {
                    WorkspaceShown::Hidden
                },
            },
        );
    Some(ToplevelInfo {
        ident,
        app: app_name(&top.app_id),
        title: WindowTitle(top.title.clone()),
        // No protocol here says which process owns a toplevel; 0 is "unknown".
        pid: Pid(0),
        geometry: top.geometry.unwrap_or(GlobalRect {
            x: GlobalCoord(0),
            y: GlobalCoord(0),
            w: Px(0),
            h: Px(0),
        }),
        workspace,
        state: top.states.clone(),
        trust: trust_of(&top.app_id),
    })
}

/// The app id as an [`AppName`]. An id that is not reverse-DNS (`konsole`, `Alacritty`) is
/// folded into `app.<id>` with every other character made `_`; an empty one is `app.unknown`.
pub(crate) fn app_name(app_id: &str) -> AppName {
    if let Ok(name) = AppName::parse(app_id) {
        return name;
    }
    let folded: String = app_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
        .collect();
    let folded = if folded.is_empty() { "unknown".to_owned() } else { folded };
    let lead = if folded.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        folded
    } else {
        format!("_{folded}")
    };
    AppName::parse(&format!("app.{lead}")).unwrap_or_else(|_| {
        AppName::parse("app.unknown").expect("a valid app name")
    })
}

/// How far cua may trust a window, from its app id alone (there is no sandbox information in
/// the protocols this reads): the shell's own windows are never leasable, other quire apps are
/// ours, everything else is native.
pub(crate) fn trust_of(app_id: &str) -> WindowTrust {
    const SHELL: [&str; 3] = ["org.quire.Shell", "org.quire.Sill", "org.quire.Desktop"];
    if SHELL.iter().any(|shell| app_id.starts_with(shell)) {
        WindowTrust::Shell
    } else if app_id.starts_with("org.quire.") {
        WindowTrust::Quire
    } else {
        WindowTrust::Native
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_ids_become_app_names() {
        let table = [
            ("org.gnome.Calculator", "org.gnome.Calculator"),
            ("konsole", "app.konsole"),
            ("Alacritty", "app.Alacritty"),
            ("", "app.unknown"),
            ("9lives", "app._9lives"),
            ("my app!", "app.my_app_"),
            ("com.system76.CosmicTerm", "com.system76.CosmicTerm"),
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
}
