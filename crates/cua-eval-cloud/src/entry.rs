//! What the eval reads from a trial catalogue entry beyond what `cua_eval::live::Plan` does: the
//! price of the OpenRouter reach, for the report.

use crate::watch::Price;
use model_catalog::{Locality, ModelEntry};

/// The OpenRouter price in the entry (the only reach the eval's account has), per million tokens
/// in micro-dollars.
pub fn price_of(entry: &ModelEntry) -> Option<Price> {
    let Locality::Remote { reach } = &entry.locality else {
        return None;
    };
    reach
        .iter()
        .find(|r| r.provider.0 == "openrouter")
        .map(|r| Price {
            input: r.price.input_per_mtok.0,
            output: r.price.output_per_mtok.0,
        })
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn every_trial_entry_is_a_computer_use_plan_with_an_openrouter_price() {
        let entries = trial_entries();
        assert!(entries.len() >= 3, "{entries:?}");
        for (file, text) in entries {
            let entry = parse_entry(&text).unwrap_or_else(|e| panic!("{file}: {e}"));
            assert!(
                Plan::of(&text, &entry.id.0, Thinking::Off).is_ok(),
                "{file} is not a computer-use plan"
            );
            assert!(price_of(&entry).is_some(), "{file} has no OpenRouter reach");
        }
    }

    #[test]
    fn the_price_is_the_reach_not_the_company() {
        let entries = trial_entries();
        let (_, text) = entries
            .iter()
            .find(|(f, _)| f.starts_with("kimi"))
            .expect("kimi");
        let entry = parse_entry(text).expect("entry");
        assert_eq!(
            price_of(&entry),
            Some(Price {
                input: 950_000,
                output: 14_000_000
            })
        );
    }
}
