//! The locator over a reader of plain values: no disk.

use super::tests::base;
use super::*;
use cua_run::Switch;
use std::path::{Path, PathBuf};

/// The files a test shows the locator: a path with its text, or `None` for a file that is there
/// and cannot be read.
struct Files(Vec<(PathBuf, Option<String>)>);

impl ReadText for Files {
    fn read_text(&self, path: &Path) -> Result<String, ReadFailure> {
        self.0
            .iter()
            .find(|(at, _)| at == path)
            .map_or(Err(ReadFailure::Missing), |(_, shown)| match shown {
                Some(text) => Ok(text.clone()),
                None => Err(ReadFailure::Unreadable {
                    source: std::io::Error::other("is a directory"),
                }),
            })
    }
}

fn locator_for(home: &str, dirs: &str) -> Locator {
    Locator::from_env(&|k: &str| match k {
        "XDG_CONFIG_HOME" => Some(home.to_owned()),
        "XDG_CONFIG_DIRS" => Some(dirs.to_owned()),
        _ => None,
    })
}

fn at(dir: &str, shown: Option<String>) -> (PathBuf, Option<String>) {
    (Path::new(dir).join(SETTINGS_FILE), shown)
}

#[test]
fn the_locator_reads_the_first_file_of_the_configuration_directories() {
    let files = Files(vec![
        at(
            "/home",
            Some("[cua.budget]\nspend_microusd = 900\n".to_owned()),
        ),
        at(
            "/etc",
            Some("[cua.budget]\nspend_microusd = 1900\n[cua]\nenabled = \"on\"\n".to_owned()),
        ),
    ]);
    let locator = locator_for("/home", "/etc");
    assert_eq!(locator.watch_dir(), Some(PathBuf::from("/home/cua")));
    let value = locator.read(&files, &base()).value;
    assert_eq!(value.config.budget.spend, porter_core::MicroUsd(900));
    // The first file wins whole: the later directory's other keys are not merged in.
    assert_eq!(value.config.enabled, Switch::Off);
    let none = Locator::from_env(&|_: &str| None::<String>);
    assert_eq!(none.read(&files, &base()), Loaded::of(base()));
    // A settings file that is there and cannot be read is reported, and the next directory
    // still answers.
    let files = Files(vec![
        at("/blocked", None),
        at(
            "/etc",
            Some("[cua.budget]\nspend_microusd = 1900\n".to_owned()),
        ),
    ]);
    let loaded = locator_for("/blocked", "/etc").read(&files, &base());
    assert_eq!(
        loaded.value.config.budget.spend,
        porter_core::MicroUsd(1900)
    );
    assert_eq!(loaded.fallbacks.len(), 1);
    assert!(matches!(loaded.fallbacks[0].why, Why::Unreadable(_)));
    assert!(loaded.fallbacks[0].key.ends_with(SETTINGS_FILE));
}
