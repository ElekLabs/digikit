//! The project scope without a current project.
use crate::*;
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};
use dkrt_sdk::OK;

#[test]
fn the_scope_is_off_without_a_current_project() {
    let mut r = Rig::new();
    let mut out = [0u8; 4];
    assert_eq!(r.p.scope_get(OWNER, b"k", &mut out), E_SET_NOKEY);
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), E_SET_OFF);
    assert_eq!(r.p.scope_remove(OWNER, b"k"), E_SET_OFF);
    // the live mirror and -1 mean no project
    r.rec(4, b"X", 1);
    r.load(4);
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), OK);
    r.load(0x80);
    assert_eq!(r.p.slot(), -1);
    assert_eq!(r.p.scope_put(OWNER, b"k", b"v"), E_SET_OFF);
    r.load(u32::MAX);
    assert_eq!(r.p.slot(), -1);
    assert!(r.p.current().is_err());
}
