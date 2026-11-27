//! The rustflags of a build without the flags that only steer profile-guided optimisation.
//!
//! PGO changes optimisation, not layout or ABI, so a PGO host accepts a plain core library and the other way
//! round: the header's flags hash (`build.rs`, `header.rs`) is made from the flags that remain. This file is
//! shared by `build.rs` (`#[path]`) and the crate's tests.

/// `encoded` is `CARGO_ENCODED_RUSTFLAGS` (flags separated by `\x1f`). The result keeps every flag except
/// `-Cprofile-use=`, `-Cprofile-generate=`, `-Cprofile-sample-use=` and `-Cllvm-args=` of a PGO option (a value that
/// starts with `-pgo-` or `-no-pgo-`), in the spellings `-Cname=value` and `-C name=value`.
#[allow(dead_code)] // used by build.rs and the tests
pub fn strip(encoded: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut parts = encoded.split('\x1f').filter(|p| !p.is_empty());
    while let Some(part) = parts.next() {
        let (separate, setting) = match part {
            "-C" => (true, parts.clone().next().unwrap_or("")),
            p => (false, p.strip_prefix("-C").unwrap_or("")),
        };
        if pgo_setting(setting) {
            if separate {
                parts.next();
            }
            continue;
        }
        kept.push(part);
    }
    kept.join("\x1f")
}

fn pgo_setting(setting: &str) -> bool {
    ["profile-use=", "profile-generate=", "profile-sample-use="]
        .iter()
        .any(|k| setting.starts_with(k))
        || setting
            .strip_prefix("llvm-args=")
            .is_some_and(|v| v.starts_with("-pgo-") || v.starts_with("-no-pgo-"))
}

#[cfg(test)]
mod tests {
    use super::strip;

    const SEP: char = '\x1f';

    #[test]
    fn pgo_flags_are_dropped_in_both_spellings() {
        let flags = [
            "-Cprofile-use=/p.profdata",
            "-Cllvm-args=-no-pgo-warn-mismatch",
            "-C",
            "profile-generate=/raw",
            "-C",
            "llvm-args=-pgo-warn-missing-function",
            "-Cprofile-sample-use=/s",
        ]
        .join(&SEP.to_string());
        assert_eq!(strip(&flags), "");
        assert_eq!(
            strip("-Cprofile-use=/a"),
            strip("-Cllvm-args=-no-pgo-warn-mismatch")
        );
    }

    #[test]
    fn other_flags_stay_in_order() {
        let flags = format!(
            "-Ctarget-cpu=native{SEP}-Cprofile-use=/p{SEP}-Copt-level=3{SEP}-Cllvm-args=-inline-threshold=900{SEP}-C{SEP}panic=abort{SEP}-Cllvm-args=-no-pgo-warn-mismatch"
        );
        assert_eq!(
            strip(&flags),
            format!(
                "-Ctarget-cpu=native{SEP}-Copt-level=3{SEP}-Cllvm-args=-inline-threshold=900{SEP}-C{SEP}panic=abort"
            )
        );
        // a plain build and a PGO build of the same flags hash alike
        assert_eq!(strip(""), "");
        assert_eq!(
            strip(&format!(
                "-Cprofile-use=/x{SEP}-Cllvm-args=-no-pgo-warn-mismatch"
            )),
            ""
        );
    }
}
