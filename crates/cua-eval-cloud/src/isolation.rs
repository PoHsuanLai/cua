//! Proof, from inside, that this process holds no key: what its environment names and what its
//! descriptors point at. Names only, never values, and never the contents of any file.

use std::path::PathBuf;

/// Words that, in an environment variable's name, mean a credential or the key store.
const SECRET_WORDS: [&str; 6] = [
    "KEY",
    "TOKEN",
    "SECRET",
    "PASSWORD",
    "ACCOUNTD",
    "OPENROUTER",
];

/// What the process looks like from inside.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Isolation {
    /// Environment variables whose names look like a credential or the key store.
    pub secret_names: Vec<String>,
    /// Open descriptors that point at a file whose name ends in `.keys` or `.key`.
    pub key_files_open: Vec<PathBuf>,
    /// How many variables the environment has.
    pub env_count: usize,
}

impl Isolation {
    /// Whether nothing of a key is in reach.
    pub fn clean(&self) -> bool {
        self.secret_names.is_empty() && self.key_files_open.is_empty()
    }

    /// One line for the log.
    pub fn line(&self) -> String {
        format!(
            "cua-eval-cloud: isolation: {} environment variables, {} with a credential-looking name, {} key file(s) open: {}",
            self.env_count,
            self.secret_names.len(),
            self.key_files_open.len(),
            if self.clean() { "clean" } else { "NOT CLEAN" }
        )
    }
}

fn looks_secret(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_WORDS.iter().any(|word| upper.contains(word))
}

fn looks_like_key_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e == "keys" || e == "key")
}

/// Looks at this process.
pub fn look() -> Isolation {
    let names: Vec<String> = std::env::vars_os()
        .map(|(k, _)| k.to_string_lossy().into_owned())
        .collect();
    let open: Vec<PathBuf> = std::fs::read_dir("/proc/self/fd")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| std::fs::read_link(entry.path()).ok())
        .filter(|target| looks_like_key_file(target))
        .collect();
    Isolation {
        env_count: names.len(),
        secret_names: names.into_iter().filter(|n| looks_secret(n)).collect(),
        key_files_open: open,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_that_look_like_credentials_are_found() {
        assert!(looks_secret("ACCOUNTD_KEYS"));
        assert!(looks_secret("openrouter_api_key"));
        assert!(!looks_secret("XDG_RUNTIME_DIR"));
    }

    #[test]
    fn a_key_file_is_told_by_its_extension() {
        assert!(looks_like_key_file(std::path::Path::new(
            "/x/accountd.keys"
        )));
        assert!(!looks_like_key_file(std::path::Path::new("/x/inferd.toml")));
    }
}
