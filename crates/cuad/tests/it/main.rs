mod binary;
mod ei_socket;
mod inflight;
mod links;
mod model;
mod serve;
mod settings;
mod supervisor;
mod support;

/// Every module file next to this one must be declared above, or its tests never run.
#[test]
fn every_test_file_is_included() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let main = include_str!("main.rs");
    let declared = |name: &str| main.lines().any(|l| l.trim() == format!("mod {name};"));
    let mut loose = Vec::new();
    for entry in std::fs::read_dir(root.join("it")).expect("tests/it") {
        let path = entry.expect("an entry").path();
        let name = path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let is_module = if path.is_dir() {
            path.join("mod.rs").exists()
        } else {
            path.extension().is_some_and(|e| e == "rs") && name != "main"
        };
        if is_module && !declared(name) {
            loose.push(name.to_owned());
        }
    }
    for entry in std::fs::read_dir(&root).expect("tests") {
        let path = entry.expect("an entry").path();
        if path.extension().is_some_and(|e| e == "rs") {
            loose.push(format!(
                "tests/{}",
                path.file_name().unwrap_or_default().to_string_lossy()
            ));
        }
    }
    assert!(
        loose.is_empty(),
        "not reached by a `mod` in tests/it/main.rs: {loose:?}"
    );
}
