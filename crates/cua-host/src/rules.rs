//! The screen rules cua ships: the phrases, window classes, commit words and sensitive titles
//! `cua-run` checks a screen against. The pure crate takes them as a [`Rules`] value; this is the
//! edge that parses the file text. The program embeds `dist/screen-rules.toml`, the installer
//! also puts a copy in the shared files as a reference, and an administrator's own
//! `screen-rules.toml` replaces the shipped one whole (the daemon reads it).

use cua_run::Rules;

/// The rules the program ships.
pub const SHIPPED_RULES: &str = include_str!("../../../dist/screen-rules.toml");

/// The file name of an administrator's replacement, under the machine's `quire` directory.
pub const RULES_FILE: &str = "screen-rules.toml";

/// The text of a rules file did not read.
#[derive(Debug, thiserror::Error)]
#[error("the screen rules do not read: {0}")]
pub struct RulesError(#[from] toml::de::Error);

/// Rules from the text of a rules file.
///
/// # Errors
///
/// [`RulesError`] when the text is not TOML or does not have the rules' shape.
pub fn parse_rules(text: &str) -> Result<Rules, RulesError> {
    Ok(toml::from_str(text)?)
}

/// The rules the program ships.
pub fn shipped_rules() -> Rules {
    parse_rules(SHIPPED_RULES).expect("the shipped screen rules read (a test holds them to it)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_rules_read_and_cover_every_table() {
        let rules = parse_rules(SHIPPED_RULES).expect("the shipped file reads");
        assert!(!rules.injection.groups.is_empty());
        assert!(!rules.injection.together.is_empty());
        assert!(!rules.injection.addressees.is_empty());
        assert!(!rules.injection.directives.is_empty());
        assert!(!rules.windows.apps.is_empty());
        assert!(!rules.windows.money_titles.is_empty());
        assert!(!rules.windows.compose_words.is_empty());
        assert!(!rules.windows.mail_apps.is_empty());
        assert!(!rules.windows.credential_titles.is_empty());
        assert!(!rules.windows.admin_titles.is_empty());
        assert!(!rules.commit.pay.is_empty());
        assert!(!rules.commit.also.is_empty());
        assert_eq!(shipped_rules(), rules);
    }

    #[test]
    fn a_file_that_is_not_rules_is_refused() {
        assert!(parse_rules("[injection]\naddressees = 3").is_err());
    }
}
