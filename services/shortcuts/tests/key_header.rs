//! The numbers of `key_abi` are those of the header of `core/input` (`key_subscribers.h`), which the image is built from.
use dkrt_svc_shortcuts::key_abi;

const HEADER: &str = include_str!("../../../runtime/platform/input/src/cf/key_subscribers.h");

/// The value of `#define name value` in the header: decimal or `0x` hexadecimal, an optional minus sign and parentheses, a `u` suffix.
fn define(name: &str) -> i64 {
    let line = HEADER
        .lines()
        .find_map(|l| {
            l.strip_prefix("#define ")?
                .strip_prefix(name)?
                .strip_prefix(' ')
        })
        .unwrap_or_else(|| panic!("{name} is not defined in the header"));
    let word = line.split_whitespace().next().unwrap();
    let t = word.trim_matches(|c| c == '(' || c == ')');
    let (neg, t) = t.strip_prefix('-').map_or((false, t), |r| (true, r));
    let t = t.trim_end_matches('u');
    let v = match t.strip_prefix("0x") {
        Some(h) => i64::from_str_radix(h, 16).unwrap(),
        None => t.parse().unwrap(),
    };
    if neg {
        -v
    } else {
        v
    }
}

#[test]
fn the_service_numbers_are_those_of_the_header() {
    assert_eq!(key_abi::MAGIC as i64, define("KEYS_INFO_MAGIC"));
    assert_eq!(key_abi::VERSION as i64, define("KEYS_INFO_VERSION"));
    assert_eq!(key_abi::FULL as i64, define("KEYS_FULL"));
    assert_eq!(key_abi::DUPLICATE as i64, define("KEYS_DUPLICATE"));
}
