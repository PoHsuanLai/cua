//! How sensitive the window a step acts in is, whatever the model says about it.
//!
//! A sensitive window makes the run ask the person before any input step, even when the router
//! answered "run" (see `machine::step::authorise`). This only adds questions: a window this
//! table does not know is `Ordinary` and the router's ruling stands alone. The derivation is
//! one pure function, [`window_sensitivity`]: the accessibility tree (a password entry), the
//! window class the app and title name, and the title. The window's trust is not a signal: the
//! shell's own windows are not administration by default, and the router rules on them itself.

use crate::window_class::window_class_in;
use a11y_tree::{A11yNode, Role, Sensitivity};
use docket_core::WindowClass;
use porter_core::AppName;
use serde::{Deserialize, Serialize};

/// The choice that lets a step in a sensitive window go on.
pub const FORCED_YES: &str = "Yes, go ahead";

/// The choice that stops it.
pub const FORCED_NO: &str = "No, stop";

/// What a window asks of the run before it is driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowSensitivity {
    /// Nothing special: the router's ruling decides.
    Ordinary,
    /// A password or another secret is asked for or kept here.
    Credentials,
    /// A system administration surface or a privilege prompt.
    Admin,
    /// Money is paid or moved here.
    Payment,
}

impl WindowSensitivity {
    /// Whether the run asks before driving this window.
    pub fn asks(self) -> bool {
        self != WindowSensitivity::Ordinary
    }

    /// The window as the question to the person names it.
    pub fn noun(self) -> &'static str {
        match self {
            WindowSensitivity::Ordinary => "a window",
            WindowSensitivity::Credentials => "a window that asks for a password",
            WindowSensitivity::Admin => "an administration window",
            WindowSensitivity::Payment => "a window that handles payments",
        }
    }
}

/// Fragments of a window title that say a password or another secret is asked for. Lower case.
const CREDENTIAL_TITLES: &[&str] = &[
    "master password",
    "enter password",
    "enter your password",
    "password required",
    "unlock vault",
    "unlock keyring",
    "unlock your",
];

/// Fragments of a window title that say a privilege is being asked for. Lower case.
const ADMIN_TITLES: &[&str] = &[
    "authentication required",
    "authorization required",
    "administrator privileges",
    "root password",
];

fn holds_secret(node: &A11yNode) -> bool {
    node.role == Role::PasswordText || node.sensitivity == Sensitivity::Secret
}

fn title_has(title: &str, fragments: &[&str]) -> bool {
    let title = title.to_lowercase();
    fragments.iter().any(|f| title.contains(f))
}

/// The sensitivity of the window of `app` titled `title`, whose
/// accessibility tree is `nodes`. A password entry on screen makes it `Credentials` first; then
/// the class the app and title name (a password manager is `Credentials`, an admin tool is
/// `Admin`, payments and banking are `Payment`); then the title's own words. Pure.
pub fn window_sensitivity(app: &AppName, title: &str, nodes: &[A11yNode]) -> WindowSensitivity {
    if nodes.iter().any(holds_secret) {
        return WindowSensitivity::Credentials;
    }
    match window_class_in(app, title, nodes) {
        WindowClass::PasswordManager => return WindowSensitivity::Credentials,
        WindowClass::Admin => return WindowSensitivity::Admin,
        WindowClass::Banking | WindowClass::Payments => return WindowSensitivity::Payment,
        WindowClass::Ordinary | WindowClass::MailCompose | WindowClass::Terminal => {}
    }
    if title_has(title, CREDENTIAL_TITLES) {
        WindowSensitivity::Credentials
    } else if title_has(title, ADMIN_TITLES) {
        WindowSensitivity::Admin
    } else {
        WindowSensitivity::Ordinary
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use a11y_tree::{NodeState, NodeValue, Parent, TypedHint};
    use cua_action::{Coord, NodeId, Point, Rect, Size};
    use prov::{Label, Labelled};
    use std::collections::BTreeSet;

    fn node(role: Role, name: &str, sensitivity: Sensitivity) -> A11yNode {
        A11yNode {
            id: NodeId(1),
            parent: Parent::Root,
            role,
            name: Labelled::new(name.to_owned(), Label::trusted_user()),
            value: NodeValue::None,
            states: BTreeSet::from([NodeState::Enabled]),
            bounds: Rect {
                origin: Point::new(Coord(0), Coord(0)),
                size: Size::new(Coord(10), Coord(10)),
            },
            actions: vec![],
            typed: TypedHint::None,
            sensitivity,
        }
    }

    #[test]
    fn the_tree_the_app_and_the_title_pick_the_sensitivity() {
        use WindowSensitivity::{Admin, Credentials, Ordinary, Payment};
        struct Row {
            app: &'static str,
            title: &'static str,
            nodes: Vec<A11yNode>,
            want: WindowSensitivity,
        }
        let row = |app, title, want| Row {
            app,
            title,
            nodes: vec![],
            want,
        };
        let rows = [
            row("org.mozilla.firefox", "Shop - Cart", Ordinary),
            row("org.mozilla.firefox", "Sign in", Ordinary),
            row("org.gnome.Terminal", "bash", Ordinary),
            row("org.quire.Mail", "New message", Ordinary),
            row("org.keepassxc.KeePassXC", "Vault", Credentials),
            row("org.example.Vault", "Vault - Master password", Credentials),
            row("org.example.Unlock", "Unlock your keyring", Credentials),
            row("org.freedesktop.PolicyKit1.Authentication", "x", Admin),
            row("org.gnome.polkit", "Dialog", Admin),
            row("org.example.AuthDialog", "Authentication required", Admin),
            row("org.example.Bank", "Account", Payment),
            row("org.mozilla.firefox", "Shop - Checkout", Payment),
            row("org.mozilla.firefox", "Bank transfer", Payment),
            row("org.quire.Shell", "Panel", Ordinary),
            Row {
                nodes: vec![node(Role::PasswordText, "", Sensitivity::Secret)],
                ..row("org.mozilla.firefox", "Account", Credentials)
            },
            Row {
                nodes: vec![node(Role::Entry, "Token", Sensitivity::Secret)],
                ..row("org.mozilla.firefox", "Account", Credentials)
            },
            Row {
                nodes: vec![node(Role::Entry, "Name", Sensitivity::Normal)],
                ..row("org.mozilla.firefox", "Account", Ordinary)
            },
        ];
        for r in rows {
            let app = AppName::parse(r.app).unwrap();
            assert_eq!(
                window_sensitivity(&app, r.title, &r.nodes),
                r.want,
                "{} {}",
                r.app,
                r.title
            );
        }
    }

    #[test]
    fn only_an_ordinary_window_does_not_ask() {
        assert!(!WindowSensitivity::Ordinary.asks());
        assert!(WindowSensitivity::Credentials.asks());
        assert!(WindowSensitivity::Admin.asks());
        assert!(WindowSensitivity::Payment.asks());
    }
}
