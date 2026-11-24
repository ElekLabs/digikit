use super::*;

#[test]
fn timeout_is_visible_without_an_acknowledgment_and_a_late_commit_reconciles() {
    let mut publisher = Publisher {
        pending: None,
        completed: Some(Completion {
            error: E_READ,
            late_generation: Some(2),
        }),
        next_generation: 3,
        sector: Sector::empty(),
    };
    assert_eq!(publisher.status(None), E_READ);
    assert_eq!(publisher.status(Some(1)), E_READ);
    assert_eq!(publisher.status(Some(2)), 2);

    publisher.completed = None;
    assert_eq!(publisher.status(None), E_BUSY);
    assert_eq!(publisher.status(Some(2)), 2);
}
