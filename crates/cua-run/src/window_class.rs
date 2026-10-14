//! What kind of window a step acts in, as far as the app's name says.
//!
//! The router makes the ruling; this only names the class it is told. The table is short on
//! purpose: a class it does not know is `Ordinary`, and the default effect table applies.

use crate::commit::{Commit, commit_of};
use a11y_tree::A11yNode;
use cua_action::WindowClass;
use porter_core::AppName;

/// Fragments of an app id that name a kind of window, first match wins. Lower case.
const APPS: &[(&str, WindowClass)] = &[
    ("terminal", WindowClass::Terminal),
    ("konsole", WindowClass::Terminal),
    ("alacritty", WindowClass::Terminal),
    ("kitty", WindowClass::Terminal),
    ("wezterm", WindowClass::Terminal),
    ("foot", WindowClass::Terminal),
    ("keepass", WindowClass::PasswordManager),
    ("bitwarden", WindowClass::PasswordManager),
    ("1password", WindowClass::PasswordManager),
    ("polkit", WindowClass::Admin),
    ("policykit", WindowClass::Admin),
    ("gparted", WindowClass::Admin),
    ("bank", WindowClass::Banking),
    ("paypal", WindowClass::Banking),
    ("venmo", WindowClass::Banking),
];

/// Fragments of a window title that say money is being paid or moved: a checkout or payment
/// form in a browser, a transfer screen. Lower case. A false positive only costs a question.
const MONEY_TITLES: &[(&str, WindowClass)] = &[
    ("checkout", WindowClass::Payments),
    ("check out", WindowClass::Payments),
    ("payment", WindowClass::Payments),
    ("billing", WindowClass::Payments),
    ("credit card", WindowClass::Payments),
    ("place order", WindowClass::Payments),
    ("pay now", WindowClass::Payments),
    ("purchase", WindowClass::Payments),
    ("online banking", WindowClass::Banking),
    ("internet banking", WindowClass::Banking),
    ("wire transfer", WindowClass::Banking),
    ("bank transfer", WindowClass::Banking),
    ("money transfer", WindowClass::Banking),
    ("transfer funds", WindowClass::Banking),
    ("send money", WindowClass::Banking),
];

/// Words in a mail window's title that say it is composing.
const COMPOSE: &[&str] = &["compose", "new message", "write:", "reply", "forward"];

/// Fragments of an app id that are mail clients.
const MAIL: &[&str] = &["mail", "thunderbird", "evolution", "geary"];

/// The class of the window of `app` titled `title`.
pub fn window_class_of(app: &AppName, title: &str) -> WindowClass {
    let id = app.as_str().to_lowercase();
    if let Some((_, class)) = APPS.iter().find(|(fragment, _)| id.contains(fragment)) {
        return *class;
    }
    let title = title.to_lowercase();
    let composing = COMPOSE.iter().any(|word| title.contains(word));
    if composing && MAIL.iter().any(|fragment| id.contains(fragment)) {
        return WindowClass::MailCompose;
    }
    match MONEY_TITLES
        .iter()
        .find(|(fragment, _)| title.contains(fragment))
    {
        Some((_, class)) => *class,
        None => WindowClass::Ordinary,
    }
}

/// The class of the window of `app` titled `title` whose accessibility tree is `nodes`: what
/// [`window_class_of`] says, and when that is `Ordinary`, `Payments` if a pay control is on
/// screen (a button named for paying, wherever the page's title is silent).
pub fn window_class_in(app: &AppName, title: &str, nodes: &[A11yNode]) -> WindowClass {
    match window_class_of(app, title) {
        WindowClass::Ordinary if nodes.iter().any(|n| commit_of(n) == Some(Commit::Pay)) => {
            WindowClass::Payments
        }
        class => class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_and_the_title_pick_the_class_and_anything_else_is_ordinary() {
        let table = [
            ("org.gnome.Terminal", "bash", WindowClass::Terminal),
            (
                "org.keepassxc.KeePassXC",
                "Vault",
                WindowClass::PasswordManager,
            ),
            (
                "org.mozilla.Thunderbird",
                "Write: Hello",
                WindowClass::MailCompose,
            ),
            ("org.mozilla.Thunderbird", "Inbox", WindowClass::Ordinary),
            (
                "org.mozilla.firefox",
                "Compose a letter",
                WindowClass::Ordinary,
            ),
            ("org.quire.Mail", "New message", WindowClass::MailCompose),
            ("org.example.Bank", "Account", WindowClass::Banking),
            (
                "org.mozilla.firefox",
                "Shop - Checkout",
                WindowClass::Payments,
            ),
            (
                "org.mozilla.firefox",
                "Payment details",
                WindowClass::Payments,
            ),
            ("org.mozilla.firefox", "Bank transfer", WindowClass::Banking),
            ("org.mozilla.firefox", "Shop - Cart", WindowClass::Ordinary),
            (
                "org.quire.Mail",
                "Reply: payment due",
                WindowClass::MailCompose,
            ),
        ];
        for (app, title, want) in table {
            assert_eq!(
                window_class_of(&AppName::parse(app).unwrap(), title),
                want,
                "{app} {title}"
            );
        }
    }
}
