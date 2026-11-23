use super::excuses_equal_loads;

const ALL: &[(&str, &str)] = &[
    ("memory", "a"),
    ("dsp", "b"),
    ("settings", "c"),
    ("state", "d"),
];

#[test]
fn only_a_service_that_declares_itself_stateless_skips_the_differ_assertion() {
    assert!(excuses_equal_loads(true, ALL));
    // an old service that lists every area keeps the full assertion
    assert!(!excuses_equal_loads(false, ALL));
    // a stateless service that leaves an area out is not excused either
    assert!(!excuses_equal_loads(true, &ALL[..3]));
}
