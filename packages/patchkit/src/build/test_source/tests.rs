use super::*;

#[test]
fn the_rule_names_test_files() {
    let rule = TestSources::get();
    for p in ["tests.rs", "a/spans_tests.rs", "tests_gen.rs", "fake_p.rs"] {
        assert!(rule.is_test_file(p), "{p}");
    }
    for p in ["lib.rs", "a/install.rs", "contest.rs", "a/test_lock.rs"] {
        assert!(!rule.is_test_file(p), "{p}");
    }
}
