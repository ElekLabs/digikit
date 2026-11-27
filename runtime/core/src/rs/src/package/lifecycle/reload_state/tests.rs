use super::ReloadState::*;
use super::*;

fn at(s: ReloadState) -> ReloadState {
    s
}

#[test]
fn a_reload_that_ends_well_is_idle_and_not_torn() {
    let mut s = at(Idle);
    s.call();
    assert!(s.is_called() && !s.is_waiting() && !s.is_torn());
    s.done();
    assert_eq!(s, Idle);
}

#[test]
fn a_pending_restore_is_waiting_and_torn_until_it_ends() {
    let mut s = at(Called);
    s.failed(true, Some(false));
    assert_eq!(s, Restoring);
    assert!(s.is_waiting() && s.is_torn());
    // the call that finishes it does not look at the members: what is known stays
    s.failed(false, None);
    assert_eq!(s, Down);
    assert!(!s.is_waiting() && s.is_torn());
}

#[test]
fn a_refusal_before_the_take_down_is_not_torn() {
    let mut s = at(Called);
    s.failed(false, Some(false));
    assert_eq!(s, Idle);
    assert!(!s.is_torn());
    let mut s = at(Called);
    s.failed(false, Some(true));
    assert_eq!(s, Down);
}

#[test]
fn a_retained_closure_waits_and_is_not_torn() {
    let mut s = at(Called);
    s.wait_retained();
    assert!(s.is_waiting() && !s.is_torn());
    s.stop_waiting();
    assert_eq!(s, Idle);
    s.wait_retained();
    s.done();
    assert_eq!(s, Idle);
}

#[test]
fn giving_up_a_restore_leaves_the_closure_down() {
    let mut s = at(Restoring);
    s.stop_waiting();
    assert_eq!(s, Down);
    s.stop_waiting();
    assert_eq!(s, Down, "idempotent");
}

#[test]
fn call_does_not_step_back_and_reset_forgets_everything() {
    let mut s = at(Down);
    s.call();
    assert_eq!(s, Down);
    s.reset();
    assert_eq!(s, Idle);
    assert!(!s.is_called());
}
