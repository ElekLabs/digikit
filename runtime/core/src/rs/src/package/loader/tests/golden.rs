//! A text golden: lines a test records, compared with a committed file. `UPDATE_GOLDENS=1` writes the file instead; otherwise a difference fails with the first differing lines.
use std::path::PathBuf;
use std::string::String;
use std::vec::Vec;

pub(super) struct Golden {
    text: String,
}

impl Golden {
    pub(super) fn new() -> Golden {
        Golden { text: String::new() }
    }

    /// A heading line that starts a section.
    pub(super) fn section(&mut self, name: &str) {
        self.text.push_str(&std::format!("== {name}\n"));
    }

    pub(super) fn line(&mut self, s: impl AsRef<str>) {
        self.text.push_str(s.as_ref());
        self.text.push('\n');
    }

    pub(super) fn contains(&self, s: &str) -> bool {
        self.text.contains(s)
    }

    /// Compare with `package/loader/tests/snapshots/<file>`, or write it when `UPDATE_GOLDENS` is set.
    pub(super) fn check(&self, file: &str) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/package/loader/tests/snapshots").join(file);
        if std::env::var_os("UPDATE_GOLDENS").is_some() {
            std::fs::write(&path, &self.text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            return;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}; run with UPDATE_GOLDENS=1", path.display()));
        if want == self.text {
            return;
        }
        let (a, b): (Vec<&str>, Vec<&str>) = (want.lines().collect(), self.text.lines().collect());
        let mut diff = String::new();
        let mut shown = 0;
        for i in 0..a.len().max(b.len()) {
            let (x, y) = (a.get(i).copied(), b.get(i).copied());
            if x != y && shown < 12 {
                diff.push_str(&std::format!("line {}:\n  want {}\n  got  {}\n", i + 1, x.unwrap_or("<end>"), y.unwrap_or("<end>")));
                shown += 1;
            }
        }
        panic!("{file} differs from the committed snapshot (UPDATE_GOLDENS=1 rewrites it):\n{diff}");
    }
}
