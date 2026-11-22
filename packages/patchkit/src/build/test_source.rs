//! What counts as a Rust unit-test source: a `#[cfg(test)]` module that a release build does not compile. It is no part of the
//! object, so the source hash leaves it out. The rule is data, `config/test-source-patterns.json`, which `tools/nx/test-source.mjs` and
//! `tools/devtools/emud_key.py` read too; `tools/nx/test-source.mjs` owns the cfg(test) guard and the Nx globs.

use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Deserialize)]
pub struct TestSources {
    dirs: Vec<String>,
    rust_files: Vec<String>,
}

/// A name pattern with at most one `*`.
fn name_matches(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((head, tail)) => {
            name.len() >= head.len() + tail.len() && name.starts_with(head) && name.ends_with(tail)
        }
    }
}

impl TestSources {
    /// The rule of `config/test-source-patterns.json`, compiled in so that a change rebuilds patchkit.
    pub fn get() -> &'static TestSources {
        static RULE: OnceLock<TestSources> = OnceLock::new();
        RULE.get_or_init(|| {
            serde_json::from_str(include_str!("../../../../config/test-source-patterns.json"))
                .expect("config/test-source-patterns.json")
        })
    }

    pub fn is_test_dir(&self, name: &str) -> bool {
        self.dirs.iter().any(|d| d == name)
    }

    /// Is the file a Rust test source by its own name (a file inside a test directory is found by the walk).
    pub fn is_test_file(&self, name: &str) -> bool {
        name.ends_with(".rs") && self.rust_files.iter().any(|p| name_matches(p, name))
    }
}

#[cfg(test)]
mod tests;
