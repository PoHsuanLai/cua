//! The person's settings file reaches a running supervisor: a change in the file applies to the
//! next start without a restart. The wait is the watch's own event, never a sleep.

use crate::support::supervisor;
use cua_bus::{ControlRefusal, CuaCaller};
use cua_desktop::BackendKind;
use cua_fake::FakePdp;
use cua_fake::scene::{click, finish, reply, task};
use cua_run::Switch;
use cuad::{Apps, CuaSettings, Locator, SettingsWatch, WatchState};
use std::path::Path;

/// What the Settings app does: a temp file, then a rename over the settings file.
fn write_atomically(home: &Path, text: &str) {
    let dir = home.join("cua");
    std::fs::create_dir_all(&dir).unwrap();
    let temp = dir.join("settings.toml.tmp");
    std::fs::write(&temp, text).unwrap();
    std::fs::rename(&temp, dir.join("settings.toml")).unwrap();
}

#[tokio::test]
async fn a_change_in_the_file_applies_to_the_next_start_without_a_restart() {
    let home = tempfile::tempdir().unwrap();
    let nothing = tempfile::tempdir().unwrap();
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some(home.path().display().to_string()),
        "XDG_CONFIG_DIRS" => Some(nothing.path().display().to_string()),
        _ => None,
    };
    let sup = supervisor(
        BackendKind::KwinStock,
        vec![reply(click()), reply(finish())],
        FakePdp::allow_all(),
    );
    // What the daemon does at start: the file over the configuration, no app on until it says.
    let base = CuaSettings {
        config: sup.settings(),
        apps: Apps::Listed(Vec::new()),
    };
    let mut watch = SettingsWatch::start(Locator::from_env(&env), base);
    assert_eq!(*watch.state(), WatchState::Live);
    sup.apply_settings(&watch.current().value);
    assert_eq!(
        sup.start(CuaCaller::Companion, task()).await,
        Err(ControlRefusal::Disabled),
        "no app is on until the person lists one"
    );

    let app = task().app;
    write_atomically(
        home.path(),
        &format!(
            "[cua]\nenabled = \"on\"\n[[cua.apps]]\napp = \"{}\"\nallow = \"ask\"\n",
            app.as_str()
        ),
    );
    while sup.apps().rule_for(&app).is_none() {
        let loaded = sup.follow(&mut watch).await.expect("the watch is live");
        assert_eq!(loaded.fallbacks, vec![]);
    }
    let started = sup.start(CuaCaller::Companion, task()).await;
    assert!(started.is_ok(), "{started:?}");
    assert_eq!(
        cuad::reach_json(sup.settings().enabled),
        r#"{"cua.run.start":"offered"}"#,
        "the live reach follows the file"
    );

    // Off again, in the file: the next start is refused, the daemon never restarted.
    write_atomically(home.path(), "[cua]\nenabled = \"off\"\n");
    while sup.settings().enabled == Switch::On {
        sup.follow(&mut watch).await.expect("the watch is live");
    }
    assert_eq!(
        sup.start(CuaCaller::Companion, task()).await,
        Err(ControlRefusal::Disabled)
    );
    assert_eq!(
        cuad::reach_json(sup.settings().enabled),
        r#"{"cua.run.start":"hidden"}"#
    );
}

#[tokio::test]
async fn a_bad_value_in_the_file_leaves_the_running_value_and_is_reported() {
    let home = tempfile::tempdir().unwrap();
    let nothing = tempfile::tempdir().unwrap();
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some(home.path().display().to_string()),
        "XDG_CONFIG_DIRS" => Some(nothing.path().display().to_string()),
        _ => None,
    };
    let sup = supervisor(BackendKind::KwinStock, vec![], FakePdp::allow_all());
    let base = CuaSettings {
        config: sup.settings(),
        apps: Apps::Unrestricted,
    };
    let mut watch = SettingsWatch::start(Locator::from_env(&env), base);
    write_atomically(home.path(), "[cua.budget]\nspend_microusd = -1\n");
    let loaded = sup.follow(&mut watch).await.expect("the watch is live");
    assert_eq!(loaded.fallbacks.len(), 1);
    assert_eq!(loaded.fallbacks[0].key, "cua.budget.spend_microusd");
    assert_eq!(sup.settings().budget.spend, porter_core::MicroUsd(0));
}

#[tokio::test]
async fn a_settings_file_that_cannot_be_read_is_reported_and_the_next_directory_answers() {
    let blocked = tempfile::tempdir().unwrap();
    let etc = tempfile::tempdir().unwrap();
    // A directory stands in for a file that is there and cannot be read.
    std::fs::create_dir_all(blocked.path().join("cua/settings.toml")).unwrap();
    write_atomically(etc.path(), "[cua.budget]\nspend_microusd = 1900\n");
    let env = |key: &str| match key {
        "XDG_CONFIG_HOME" => Some(blocked.path().display().to_string()),
        "XDG_CONFIG_DIRS" => Some(etc.path().display().to_string()),
        _ => None,
    };
    let sup = supervisor(BackendKind::KwinStock, vec![], FakePdp::allow_all());
    let base = CuaSettings {
        config: sup.settings(),
        apps: Apps::Unrestricted,
    };
    let loaded = SettingsWatch::start(Locator::from_env(&env), base).current();
    assert_eq!(
        loaded.value.config.budget.spend,
        porter_core::MicroUsd(1900)
    );
    assert_eq!(loaded.fallbacks.len(), 1);
    assert!(matches!(loaded.fallbacks[0].why, cuad::Why::Unreadable(_)));
}
