use super::ReloadState::*;
use super::*;

/// A carry with nothing held and an arena that has nothing to give: these tests are about the states (`carried_state/tests.rs` holds the blocks' side).
struct NoBlocks;

impl Blocks for NoBlocks {
    unsafe fn alloc(&mut self, _: u32) -> Option<crate::package::loader::Block> {
        None
    }
    unsafe fn free(&mut self, _: dkrt_abi::dev_addr::DevAddr) {}
}

fn nothing() -> (CarriedState, NoBlocks) {
    (CarriedState::NEW, NoBlocks)
}

fn at(s: ReloadState) -> ReloadState {
    s
}

#[test]
fn a_reload_that_ends_well_is_idle_and_not_torn() {
    let (mut c, mut a) = nothing();
    let mut s = at(Idle);
    s.call();
    assert!(s.is_called() && !s.is_waiting() && !s.is_torn());
    unsafe { s.done(&mut c, &mut a) };
    assert_eq!(s, Idle);
}

#[test]
fn a_pending_restore_is_waiting_and_torn_until_it_ends() {
    let (mut c, mut a) = nothing();
    let mut s = at(Called);
    unsafe { s.failed(true, Some(false), &mut c, &mut a) };
    assert_eq!(s, Restoring);
    assert!(s.is_waiting() && s.is_torn());
    // the call that finishes it does not look at the members: what is known stays
    unsafe { s.failed(false, None, &mut c, &mut a) };
    assert_eq!(s, Down);
    assert!(!s.is_waiting() && s.is_torn());
}

#[test]
fn a_refusal_before_the_take_down_is_not_torn() {
    let (mut c, mut a) = nothing();
    let mut s = at(Called);
    unsafe { s.failed(false, Some(false), &mut c, &mut a) };
    assert_eq!(s, Idle);
    assert!(!s.is_torn());
    let mut s = at(Called);
    unsafe { s.failed(false, Some(true), &mut c, &mut a) };
    assert_eq!(s, Down);
}

#[test]
fn a_retained_closure_waits_and_is_not_torn() {
    let (mut c, mut a) = nothing();
    let mut s = at(Called);
    unsafe { s.wait_retained(&mut c, &mut a) };
    assert!(s.is_waiting() && !s.is_torn());
    unsafe { s.stop_waiting(&mut c, &mut a) };
    assert_eq!(s, Idle);
    unsafe { s.wait_retained(&mut c, &mut a) };
    unsafe { s.done(&mut c, &mut a) };
    assert_eq!(s, Idle);
}

#[test]
fn giving_up_a_restore_leaves_the_closure_down() {
    let (mut c, mut a) = nothing();
    let mut s = at(Restoring);
    unsafe { s.stop_waiting(&mut c, &mut a) };
    assert_eq!(s, Down);
    unsafe { s.stop_waiting(&mut c, &mut a) };
    assert_eq!(s, Down, "idempotent");
}

#[test]
fn call_does_not_step_back_and_reset_forgets_everything() {
    let (mut c, mut a) = nothing();
    let mut s = at(Down);
    s.call();
    assert_eq!(s, Down);
    unsafe { s.reset(&mut c, &mut a) };
    assert_eq!(s, Idle);
    assert!(!s.is_called());
}
