//! The fixture files load, equal the snapshots built beside them, and survive a round trip.
//! Regenerate with `cargo test -p a11y-tree -- --ignored regenerate_fixture_files`, and only
//! with a reason.

use a11y_tree::A11ySnapshot;
use std::path::PathBuf;

fn path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures/a11y")
        .join(format!("{name}.json"))
}

fn all() -> Vec<(&'static str, A11ySnapshot)> {
    vec![
        ("gtk4_dialog", crate::support::gtk4_dialog()),
        ("firefox_form", crate::support::firefox_form()),
        ("quire_mail", crate::support::quire_mail()),
    ]
}

#[test]
fn fixture_files_load_and_equal_the_snapshots_built_beside_them() {
    for (name, built) in all() {
        let text = std::fs::read_to_string(path(name)).unwrap_or_else(|e| panic!("{name}: {e}"));
        let loaded: A11ySnapshot =
            serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(loaded, built, "{name}");
        let again = serde_json::to_string_pretty(&loaded).unwrap();
        assert_eq!(
            again.trim_end(),
            text.trim_end(),
            "{name} is the canonical serde form"
        );
    }
}

#[test]
fn every_fixture_node_is_labelled_untrusted_screen_text_and_the_root_is_first() {
    for (name, snapshot) in all() {
        assert_eq!(snapshot.nodes[0].parent, a11y_tree::Parent::Root, "{name}");
        for node in &snapshot.nodes {
            assert_eq!(
                node.name.label.integrity,
                prov::Integrity::Untrusted,
                "{name} {:?}",
                node.id
            );
        }
    }
}

#[test]
fn the_password_fixture_is_marked_secret_and_nothing_else_is() {
    let form = crate::support::firefox_form();
    let secret: Vec<_> = form
        .nodes
        .iter()
        .filter(|n| n.sensitivity == a11y_tree::Sensitivity::Secret)
        .map(|n| n.role)
        .collect();
    assert_eq!(secret, vec![a11y_tree::Role::PasswordText]);
}

#[test]
#[ignore = "writes the fixture files; run on purpose"]
fn regenerate_fixture_files() {
    for (name, built) in all() {
        let mut text = serde_json::to_string_pretty(&built).unwrap();
        text.push('\n');
        std::fs::write(path(name), text).unwrap();
    }
}
