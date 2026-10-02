//! Bus names and object paths.

use prov::RunId;

/// cuad's bus name.
pub const CUA_BUS: &str = "org.quire.Cua1";
/// cuad's root object (`Manager`).
pub const CUA_PATH: &str = "/org/quire/Cua1";
/// The bus name cuad also owns, as the provider of the `cua.run.start` action
/// (`org.quire.IntentProvider1` at docket's `PROVIDER_PATH`).
pub const PROVIDER_BUS: &str = "org.quire.Cua";

/// The object path of one run (`/org/quire/Cua1/run/<id>`). A run id is a slug with `.`, `_` and
/// `-`, of which an object path allows only `_`, so `.` and `-` become `_`.
pub fn run_path(run: &RunId) -> String {
    let segment: String = run
        .as_str()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{CUA_PATH}/run/{segment}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_paths_are_valid_object_paths() {
        for (id, want) in [
            ("r-1", "/org/quire/Cua1/run/r_1"),
            ("r-4.a", "/org/quire/Cua1/run/r_4_a"),
        ] {
            let path = run_path(&RunId::parse(id).unwrap());
            assert_eq!(path, want);
            assert!(zbus::zvariant::ObjectPath::try_from(path.as_str()).is_ok());
        }
    }
}
