//! The guard of a firmware call: `guard.rs` is compiled into this test by path (the crate's own `lib.rs` pulls in the assembler, which a host cannot assemble).
use std::cell::Cell;

#[path = "../src/guard.rs"]
mod guard;
use guard::call_if_bound;

#[test]
fn a_zero_address_is_refused_and_gives_the_neutral_answer() {
    let called = Cell::new(false);
    let r = call_if_bound(0, 7u32, || {
        called.set(true);
        1
    });
    assert_eq!(r, 7);
    assert!(
        !called.get(),
        "the firmware function was called at address 0"
    );
}

#[test]
fn a_bound_address_is_called() {
    assert_eq!(call_if_bound(0x4011_e150, 7u32, || 1), 1);
}
