use super::*;

/// The state after some rows were removed and logged: rows 1 and 3 read MISSING, rows 3 and 4 are logged, two tries and five busy passes were counted.
fn dirty() -> Parking {
    let mut p = Parking::new();
    p.row_removed(1, true);
    p.row_removed(3, true);
    p.tries = 2;
    p.logged = 1 << 3 | 1 << 4;
    p.busy_passes = 5;
    p
}

#[test]
fn a_removed_row_reads_missing_only_when_asked_and_the_tries_start_again() {
    let mut p = Parking::new();
    p.tries = 2;
    p.row_removed(2, false);
    assert_eq!((p.missing_rows(), p.tries()), (0, 0));
    p.tries = 2;
    p.row_removed(2, true);
    assert_eq!((p.missing_rows(), p.tries()), (1 << 2, 0));
}

#[test]
fn a_filled_row_is_no_placeholder_and_has_nothing_logged() {
    let mut p = dirty();
    p.row_filled(3);
    assert_eq!(p.missing_rows(), 1 << 1);
    assert_eq!(p.logged(), 1 << 4);
    p.row_filled(3);
    assert_eq!(
        (p.missing_rows(), p.logged()),
        (1 << 1, 1 << 4),
        "once is as twice"
    );
}

#[test]
fn the_end_of_a_reload_forgets_the_waiting_rows_and_the_tries_only() {
    let mut p = dirty();
    p.reload_over();
    assert_eq!((p.missing_rows(), p.tries()), (0, 0));
    assert_eq!((p.logged(), p.busy_passes()), (1 << 3 | 1 << 4, 5));
}

#[test]
fn a_new_plan_forgets_the_passes_and_the_log_but_not_the_waiting_rows() {
    let mut p = dirty();
    p.plan_changed();
    assert_eq!((p.tries(), p.busy_passes(), p.logged()), (0, 0, 0));
    assert_eq!(
        p.missing_rows(),
        1 << 1 | 1 << 3,
        "a row that waits for its new version still waits"
    );
}
