//! Who is calling: derived from the connection, never sent in a body.

use cua_bus::CuaCaller;
use serde::{Deserialize, Serialize};

/// The well-known bus names that stand for each caller. A caller is whoever owns one of them
/// (cuad checks the unique name of the connection against the owner of the well-known name).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerNames {
    /// The router.
    pub router: Vec<String>,
    /// The planner.
    pub companion: Vec<String>,
    /// The shell.
    pub shell: Vec<String>,
}

impl Default for CallerNames {
    fn default() -> Self {
        Self {
            router: vec!["org.quire.Intents1".to_owned()],
            companion: vec!["org.quire.Companion1".to_owned()],
            shell: vec!["org.quire.Shell".to_owned()],
        }
    }
}

impl CallerNames {
    /// Which caller owns `owned`: the first of router, planner, shell whose well-known name the
    /// connection owns; `Other` when it owns none. Unique names (`:1.9`) never match.
    pub fn classify(&self, owned: &[String]) -> CuaCaller {
        let owns = |wanted: &[String]| wanted.iter().any(|w| owned.contains(w));
        if owns(&self.router) {
            CuaCaller::Router
        } else if owns(&self.companion) {
            CuaCaller::Companion
        } else if owns(&self.shell) {
            CuaCaller::Shell
        } else {
            CuaCaller::Other
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_caller_is_whoever_owns_the_well_known_name() {
        let names = CallerNames::default();
        let owned = |list: &[&str]| list.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        let cases = [
            (
                "the router",
                owned(&["org.quire.Intents1"]),
                CuaCaller::Router,
            ),
            (
                "the planner",
                owned(&["org.quire.Companion1"]),
                CuaCaller::Companion,
            ),
            (
                "the shell",
                owned(&["org.quire.Shell", ":1.44"]),
                CuaCaller::Shell,
            ),
            ("a stranger", owned(&["org.example.Evil"]), CuaCaller::Other),
            ("nobody", owned(&[]), CuaCaller::Other),
            ("a unique name only", owned(&[":1.9"]), CuaCaller::Other),
        ];
        for (name, owned, want) in cases {
            assert_eq!(names.classify(&owned), want, "{name}");
        }
    }
}
