//! What kind of window a step acts in, as far as the app's name says.
//!
//! The router makes the ruling; this only names the class it is told. The table is short on
//! purpose: a class it does not know is `Ordinary`, and the default effect table applies.

use docket_core::WindowClass;
use porter_core::AppName;

/// Fragments of an app id that name a kind of window, first match wins. Lower case.
const APPS: &[(&str, WindowClass)] = &[
    ("terminal", WindowClass::Terminal),
    ("cosmicterm", WindowClass::Terminal),
    ("konsole", WindowClass::Terminal),
    ("alacritty", WindowClass::Terminal),
    ("kitty", WindowClass::Terminal),
    ("wezterm", WindowClass::Terminal),
    ("foot", WindowClass::Terminal),
    ("keepass", WindowClass::PasswordManager),
    ("bitwarden", WindowClass::PasswordManager),
    ("1password", WindowClass::PasswordManager),
    ("polkit", WindowClass::Admin),
    ("gparted", WindowClass::Admin),
    ("bank", WindowClass::Banking),
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
    WindowClass::Ordinary
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_and_the_title_pick_the_class_and_anything_else_is_ordinary() {
        let table = [
            ("org.gnome.Terminal", "bash", WindowClass::Terminal),
            ("com.system76.CosmicTerm", "~", WindowClass::Terminal),
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
