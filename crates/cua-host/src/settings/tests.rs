//! The schema, the key table and the reader hold to one another.

use super::keys::{APPS_KEY, Rule, table};
use super::*;
use cua_run::{DefaultMode, Switch};
use porter_core::AppName;
use std::collections::BTreeSet;

fn schema() -> toml::Table {
    SCHEMA.parse().expect("the schema is TOML")
}

fn rows() -> Vec<toml::Table> {
    schema()["key"]
        .as_array()
        .expect("key tables")
        .iter()
        .map(|k| k.as_table().expect("a table").clone())
        .collect()
}

fn path(row: &toml::Table) -> &str {
    row["path"].as_str().expect("path")
}

fn kind_of(row: &toml::Table) -> (&str, &toml::Table) {
    let kind = row["kind"].as_table().expect("kind");
    (
        kind["kind"].as_str().expect("kind name"),
        kind.get("v")
            .and_then(toml::Value::as_table)
            .expect("kind v"),
    )
}

pub(super) fn base() -> CuaSettings {
    CuaSettings {
        config: cua_run::CuaConfig::default(),
        apps: Apps::Listed(Vec::new()),
    }
}

/// A document with one key set to `value`.
fn doc(path: &str, value: toml::Value) -> String {
    let mut table = toml::Table::new();
    let mut parts: Vec<&str> = path.split('.').collect();
    let leaf = parts.pop().expect("a leaf");
    let mut cursor = &mut table;
    for part in parts {
        cursor = cursor
            .entry(part)
            .or_insert_with(|| toml::Value::Table(toml::Table::new()))
            .as_table_mut()
            .expect("a table");
    }
    cursor.insert(leaf.to_owned(), value);
    table.to_string()
}

fn words(v: &toml::Table) -> Vec<&str> {
    v["variants"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(toml::Value::as_str)
        .collect()
}

#[test]
fn the_schema_has_the_shape_the_settings_app_loads() {
    let schema = schema();
    assert_eq!(schema["app"].as_str(), Some("cua"));
    assert_eq!(schema["file"].as_str(), Some(SETTINGS_FILE));
    assert_eq!(schema["version"].as_integer(), Some(1));
    let mut seen = BTreeSet::new();
    for row in rows() {
        let p = path(&row);
        assert!(seen.insert(p.to_owned()), "duplicate {p}");
        assert!(p.starts_with("cua."), "{p}");
        for field in ["label", "help", "section"] {
            assert!(!row[field].as_str().expect(field).is_empty(), "{p} {field}");
        }
        assert!(
            matches!(row["exposure"].as_str(), Some("basic" | "advanced")),
            "{p}"
        );
        assert_eq!(row["page"]["kind"].as_str(), Some("intelligence"), "{p}");
        assert!(row.get("agent").is_none(), "{p} is never agent-settable");
        match kind_of(&row) {
            ("bounded", v) => {
                let (min, max) = (
                    v["min"].as_integer().unwrap(),
                    v["max"].as_integer().unwrap(),
                );
                let default = row["default"].as_integer().expect("a number default");
                assert!((min..=max).contains(&default), "{p}");
            }
            (kind @ ("toggle" | "segmented"), v) => {
                let words = words(v);
                assert_eq!(words.len() == 2, kind == "toggle", "{p}");
                assert!(
                    words.contains(&row["default"].as_str().expect("a word default")),
                    "{p}"
                );
                let labels = row["labels"].as_table().expect("labels for words");
                assert!(words.iter().all(|w| labels.contains_key(*w)), "{p}");
            }
            ("rows", v) => {
                let columns = v["columns"].as_array().expect("columns");
                assert_eq!(columns.len(), 2, "{p}");
                assert_eq!(row["default"].as_array().map(Vec::len), Some(0), "{p}");
            }
            other => panic!("{p}: kind {other:?}"),
        }
    }
}

#[test]
fn the_intelligence_page_shows_computer_use_and_the_rest_is_advanced() {
    let shown: Vec<String> = rows()
        .iter()
        .filter(|r| r["exposure"].as_str() == Some("basic"))
        .map(|r| path(r).to_owned())
        .collect();
    assert_eq!(
        shown,
        [
            "cua.enabled",
            "cua.apps",
            "cua.default_mode",
            "cua.autopause",
            "cua.accessibility"
        ]
    );
}

#[test]
fn the_schema_rows_and_the_key_table_are_the_same_keys_with_the_same_ranges() {
    let keys = table();
    let rows = rows();
    let scalar: Vec<&toml::Table> = rows.iter().filter(|r| path(r) != APPS_KEY).collect();
    assert_eq!(
        scalar.iter().map(|r| path(r)).collect::<Vec<_>>(),
        keys.iter().map(|k| k.path).collect::<Vec<_>>()
    );
    assert!(
        rows.iter().any(|r| path(r) == APPS_KEY),
        "the list key has its row"
    );
    for (row, key) in scalar.iter().zip(&keys) {
        let (_, v) = kind_of(row);
        match &key.rule {
            Rule::Number { range, .. } => {
                assert_eq!(v["min"].as_integer(), Some(*range.start()), "{}", key.path);
                assert_eq!(v["max"].as_integer(), Some(*range.end()), "{}", key.path);
            }
            Rule::Word { words: w, .. } => assert_eq!(&words(v), w, "{}", key.path),
        }
    }
    // The apps rows' `allow` column is the words the reader knows.
    let apps = rows.iter().find(|r| path(r) == APPS_KEY).unwrap();
    let columns = apps["kind"]["v"]["columns"].as_array().unwrap();
    let allow = columns[1]["kind"]["v"]["variants"].as_array().unwrap();
    let allow: Vec<&str> = allow.iter().filter_map(toml::Value::as_str).collect();
    assert_eq!(allow, AppAllow::WORDS);
}

fn merge(into: &mut toml::Table, key: String, value: toml::Value) {
    match (into.get_mut(&key), value) {
        (Some(toml::Value::Table(have)), toml::Value::Table(more)) => {
            for (k, v) in more {
                merge(have, k, v);
            }
        }
        (_, value) => {
            into.insert(key, value);
        }
    }
}

fn merged(parts: &[(&str, toml::Value)]) -> String {
    let mut text = toml::Table::new();
    for (p, v) in parts {
        for (k, v) in doc(p, v.clone()).parse::<toml::Table>().unwrap() {
            merge(&mut text, k, v);
        }
    }
    text.to_string()
}

#[test]
fn a_file_of_every_schema_default_changes_nothing() {
    let parts: Vec<(String, toml::Value)> = rows()
        .iter()
        .map(|r| (path(r).to_owned(), r["default"].clone()))
        .collect();
    let refs: Vec<(&str, toml::Value)> =
        parts.iter().map(|(p, v)| (p.as_str(), v.clone())).collect();
    let loaded = read(&merged(&refs), &base());
    assert_eq!(loaded.value, base());
    assert_eq!((loaded.fallbacks, loaded.unknown), (vec![], vec![]));
}

/// A valid value for the scalar row that is not its default.
fn other_than_default(row: &toml::Table) -> toml::Value {
    match kind_of(row) {
        (_, v) if v.contains_key("min") => {
            let default = row["default"].as_integer().unwrap();
            let (min, max) = (
                v["min"].as_integer().unwrap(),
                v["max"].as_integer().unwrap(),
            );
            toml::Value::Integer(if default == max { min } else { max })
        }
        (_, v) => {
            let default = row["default"].as_str().unwrap();
            let word = words(v).into_iter().find(|w| *w != default).unwrap();
            toml::Value::String(word.to_owned())
        }
    }
}

fn app_row(app: &str, allow: &str) -> toml::Value {
    let mut t = toml::Table::new();
    t.insert("app".into(), toml::Value::String(app.into()));
    t.insert("allow".into(), toml::Value::String(allow.into()));
    toml::Value::Table(t)
}

#[test]
fn every_schema_key_is_read_by_the_daemon() {
    for row in rows() {
        let p = path(&row);
        let value = if p == APPS_KEY {
            toml::Value::Array(vec![app_row("org.gnome.Calculator", "always")])
        } else {
            other_than_default(&row)
        };
        let loaded = read(&doc(p, value), &base());
        assert_eq!(loaded.fallbacks, vec![], "{p}");
        assert_ne!(loaded.value, base(), "{p} changed nothing");
    }
}

#[test]
fn each_key_lands_in_the_typed_value_the_design_names() {
    use porter_core::MicroUsd;
    let one = |p: &str, v: toml::Value| read(&doc(p, v), &base()).value.config;
    let int = toml::Value::Integer;
    let word = |w: &str| toml::Value::String(w.to_owned());
    assert_eq!(one("cua.enabled", word("on")).enabled, Switch::On);
    assert_eq!(
        one("cua.default_mode", word("agent_workspace")).default_mode,
        DefaultMode::AgentWorkspace
    );
    assert_eq!(one("cua.autopause", word("off")).autopause, Switch::Off);
    assert_eq!(
        one("cua.accessibility", word("off")).accessibility,
        Switch::Off
    );
    assert_eq!(
        one("cua.budget.spend_microusd", int(2_500_000))
            .budget
            .spend,
        MicroUsd(2_500_000)
    );
}

#[test]
fn a_bad_value_falls_back_for_that_key_alone() {
    for row in rows().iter().filter(|r| path(r) != APPS_KEY) {
        let p = path(row);
        let bad: Vec<toml::Value> = match kind_of(row) {
            (_, v) if v.contains_key("min") => vec![
                toml::Value::Integer(v["min"].as_integer().unwrap() - 1),
                toml::Value::Integer(v["max"].as_integer().unwrap() + 1),
                toml::Value::String("many".into()),
                toml::Value::Boolean(true),
            ],
            _ => vec![
                toml::Value::String("reckless".into()),
                toml::Value::Integer(1),
                toml::Value::Boolean(true),
            ],
        };
        for value in bad {
            // A second, good key in the same file still applies.
            let other = if p == "cua.budget.spend_microusd" {
                "cua.enabled"
            } else {
                "cua.budget.spend_microusd"
            };
            let good = if other == "cua.enabled" {
                toml::Value::String("on".into())
            } else {
                toml::Value::Integer(700)
            };
            let text = merged(&[(p, value.clone()), (other, good)]);
            let loaded = read(&text, &base());
            let mut want = base();
            if other == "cua.enabled" {
                want.config.enabled = Switch::On;
            } else {
                want.config.budget.spend = porter_core::MicroUsd(700);
            }
            assert_eq!(loaded.value, want, "{p} = {value}");
            assert_eq!(loaded.fallbacks.len(), 1, "{p} = {value}");
            assert_eq!(loaded.fallbacks[0].key, p);
        }
    }
}

#[test]
fn a_fallback_is_the_base_value_not_the_default() {
    let mut start = base();
    start.config.budget.spend = porter_core::MicroUsd(11);
    let loaded = read("[cua.budget]\nspend_microusd = -1\n", &start);
    assert_eq!(loaded.value, start);
    assert_eq!(
        loaded.fallbacks,
        vec![Fallback {
            key: "cua.budget.spend_microusd".into(),
            why: Why::OutOfRange {
                min: 0,
                max: 1_000_000_000,
            },
        }]
    );
}

#[test]
fn a_file_that_is_not_toml_keeps_every_base_value_and_says_so() {
    let loaded = read("[cua\nenabled = \"on\"", &base());
    assert_eq!(loaded.value, base());
    assert!(matches!(loaded.fallbacks[0].why, Why::NotToml(_)));
}

#[test]
fn unknown_keys_are_reported_and_the_retired_screens_keys_are_among_them() {
    let loaded = read(
        "version = 1\n[cua.screens]\nkeep = \"on\"\n[cua]\nenabled = \"on\"\n",
        &base(),
    );
    assert_eq!(loaded.value.config.enabled, Switch::On);
    assert_eq!(loaded.unknown, ["cua.screens.keep"]);
    assert_eq!(
        loaded.lines("cuad"),
        ["cuad: settings: cua.screens.keep: no such key, ignored"]
    );
}

fn app(id: &str) -> AppName {
    AppName::parse(id).unwrap()
}

#[test]
fn the_apps_list_reads_row_by_row_and_a_browser_is_never_always() {
    let text = r#"
[[cua.apps]]
app = "org.gnome.Calculator"
allow = "always"

[[cua.apps]]
app = "org.mozilla.firefox"
allow = "always"

[[cua.apps]]
app = "not an id"
allow = "ask"

[[cua.apps]]
app = "org.gnome.TextEditor"
allow = "sometimes"

[[cua.apps]]
app = "org.gnome.Calculator"
allow = "session"
"#;
    let loaded = read(text, &base());
    assert_eq!(
        loaded.value.apps,
        Apps::Listed(vec![
            AppRule {
                app: app("org.mozilla.firefox"),
                allow: AppAllow::Ask
            },
            AppRule {
                app: app("org.gnome.Calculator"),
                allow: AppAllow::Ask
            },
        ])
    );
    let refused: Vec<(&str, &Why)> = loaded
        .fallbacks
        .iter()
        .map(|f| (f.key.as_str(), &f.why))
        .collect();
    assert_eq!(
        refused,
        [
            ("cua.apps[1]", &Why::BrowserNeverAlways),
            ("cua.apps[2]", &Why::NotAnApp),
            ("cua.apps[3]", &Why::NotAnAllow),
        ]
    );
}

#[test]
fn no_apps_key_leaves_the_base_list_and_a_non_list_falls_back() {
    assert_eq!(
        read("[cua]\nenabled = \"on\"\n", &base()).value.apps,
        Apps::Listed(vec![])
    );
    let loaded = read("[cua]\napps = \"all\"\n", &base());
    assert_eq!(loaded.value.apps, Apps::Listed(vec![]));
    assert_eq!(loaded.fallbacks[0].why, Why::NotAList);
}

#[test]
fn only_a_listed_app_has_a_rule_and_code_built_supervisors_are_unrestricted() {
    let listed = Apps::Listed(vec![AppRule {
        app: app("org.gnome.Calculator"),
        allow: AppAllow::Ask,
    }]);
    assert_eq!(
        listed.rule_for(&app("org.gnome.Calculator")),
        Some(AppAllow::Ask)
    );
    assert_eq!(listed.rule_for(&app("org.gnome.TextEditor")), None);
    assert_eq!(
        Apps::Listed(vec![]).rule_for(&app("org.gnome.Calculator")),
        None
    );
    assert_eq!(
        Apps::Unrestricted.rule_for(&app("org.gnome.Calculator")),
        Some(AppAllow::Ask)
    );
}

#[test]
fn browsers_are_told_by_their_ids() {
    for id in [
        "org.mozilla.firefox",
        "com.google.Chrome",
        "org.chromium.Chromium",
        "com.brave.Browser",
        "org.gnome.Epiphany",
    ] {
        assert!(is_browser(&app(id)), "{id}");
    }
    for id in [
        "org.gnome.Calculator",
        "org.gnome.TextEditor",
        "org.quire.Mail",
    ] {
        assert!(!is_browser(&app(id)), "{id}");
    }
}
