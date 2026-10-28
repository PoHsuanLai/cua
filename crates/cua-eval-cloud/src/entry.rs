//! The trial catalogue entries: the tests hold each one to a computer-use plan.

mod tests {
    use cua_eval::live::{Plan, Thinking};
    use model_catalog::parse_entry;

    fn trial_entries() -> Vec<(String, String)> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../eval/cloud-catalog");
        let mut found: Vec<(String, String)> = std::fs::read_dir(dir)
            .expect("the trial catalogue")
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "toml"))
            .map(|e| {
                (
                    e.file_name().to_string_lossy().into_owned(),
                    std::fs::read_to_string(e.path()).expect("entry"),
                )
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn every_trial_entry_is_a_computer_use_plan() {
        let entries = trial_entries();
        assert!(entries.len() >= 3, "{entries:?}");
        for (file, text) in entries {
            let entry = parse_entry(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(
                Plan::of(&text, &entry.id.0, Thinking::Off).is_ok(),
                "{file} is not a computer-use plan"
            );
        }
    }
}
