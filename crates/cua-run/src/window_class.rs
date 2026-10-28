//! What kind of window a step acts in, as far as the app's name says.
//!
//! The router makes the ruling; this only names the class it is told. The tables are
//! [`WindowRules`] (shipped data) and are short on purpose: a class they do not know is
//! `Ordinary`, and the default effect table applies.

use crate::commit::pays;
use crate::rules::{ClassFragments, Rules, WindowRules};
use a11y_tree::A11yNode;
use cua_action::WindowClass;
use porter_core::AppName;

/// The class of the first row of `rows` with a fragment `text` holds.
fn class_in(rows: &[ClassFragments], text: &str) -> Option<WindowClass> {
    rows.iter()
        .find(|row| row.fragments.iter().any(|f| text.contains(f.as_str())))
        .map(|row| row.class)
}

/// The class of the window of `app` titled `title`.
pub fn window_class_of(rules: &WindowRules, app: &AppName, title: &str) -> WindowClass {
    let id = app.as_str().to_lowercase();
    if let Some(class) = class_in(&rules.apps, &id) {
        return class;
    }
    let title = title.to_lowercase();
    let composing = rules
        .compose_words
        .iter()
        .any(|word| title.contains(word.as_str()));
    if composing
        && rules
            .mail_apps
            .iter()
            .any(|fragment| id.contains(fragment.as_str()))
    {
        return WindowClass::MailCompose;
    }
    class_in(&rules.money_titles, &title).unwrap_or(WindowClass::Ordinary)
}

/// The class of the window of `app` titled `title` whose accessibility tree is `nodes`: what
/// [`window_class_of`] says, and when that is `Ordinary`, `Payments` if a pay control is on
/// screen (a button named for paying, wherever the page's title is silent).
pub fn window_class_in(
    rules: &Rules,
    app: &AppName,
    title: &str,
    nodes: &[A11yNode],
) -> WindowClass {
    match window_class_of(&rules.windows, app, title) {
        WindowClass::Ordinary if nodes.iter().any(|n| pays(&rules.commit, n)) => {
            WindowClass::Payments
        }
        class => class,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::shipped;

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
                window_class_of(&shipped().windows, &AppName::parse(app).unwrap(), title),
                want,
                "{app} {title}"
            );
        }
    }
}
