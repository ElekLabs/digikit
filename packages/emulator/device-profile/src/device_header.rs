//! Whether a line of a `devices/*.toml` file is the `[device]` table header. Shared by `build.rs` (which embeds the device files) and
//! the crate's tests (`#[path]` in both), so the check is tested. TOML allows white space around the header and a comment after it.

pub fn is_device_header(line: &str) -> bool {
    line.trim()
        .strip_prefix("[device]")
        .is_some_and(|rest| rest.trim_start().is_empty() || rest.trim_start().starts_with('#'))
}

#[cfg(test)]
mod tests {
    use super::is_device_header;

    #[test]
    fn the_header_may_have_white_space_or_a_comment_after_it() {
        for yes in [
            "[device]",
            "[device]  ",
            "[device] # the device",
            "  [device]\t",
            "[device]#x",
        ] {
            assert!(is_device_header(yes), "{yes:?}");
        }
        for no in [
            "[device.x]",
            "[devices]",
            "# [device]",
            "[device] x",
            "device",
            "[[device]]",
            "",
        ] {
            assert!(!is_device_header(no), "{no:?}");
        }
    }
}
