//! Owner and page index claims.
use super::*;

#[test]
fn replacement_pairs_are_complete_unique_unmixed_and_bounded() {
    let mut pages = PageSet::<2>::new();
    assert!(pages.claim(KIND_V2, 10, 0));
    assert!(!pages.complete());
    assert!(!pages.claim(KIND, 10, 1));
    assert!(!pages.claim(KIND_V2, 10, 0));
    assert!(!pages.claim(KIND_V2, 10, 2));
    assert!(pages.claim(KIND_V2, 10, 1));
    assert!(pages.complete());
    assert!(pages.claim(KIND, 20, 1));
    assert!(pages.complete());
    assert!(!pages.claim(KIND, 30, 1));
}
