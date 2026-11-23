extern crate std;

use super::*;
use crate::provider::{LifecycleV1, PartRef, ProviderRow, PART_LIFECYCLE};
use crate::service::SettleCtx;
use core::sync::atomic::{AtomicU32, Ordering};
use dkrt_sdk::E_MISSING;
use std::boxed::Box;
use std::vec::Vec;

static ATTACHED: AtomicU32 = AtomicU32::new(0);
static DETACHED: AtomicU32 = AtomicU32::new(0);

/// Instance byte 1: refuses its attach.
unsafe extern "C" fn attach(i: *mut c_void, _: *const c_void, _: *const ApplyCtx) -> i32 {
    ATTACHED.fetch_add(1, Ordering::SeqCst);
    if *(i as *mut u8) == 1 {
        E_MISSING
    } else {
        OK
    }
}
unsafe extern "C" fn detach(_: *mut c_void, _: *const c_void, _: u8) {
    DETACHED.fetch_add(1, Ordering::SeqCst);
}
unsafe extern "C" fn can_stop(_: *mut c_void, _: *const c_void) -> i32 {
    OK
}
unsafe extern "C" fn settle(_: *mut c_void, _: *const c_void, _: *const SettleCtx) {}
unsafe extern "C" fn reload_done(_: *mut c_void, _: *const c_void) {}

static LIFECYCLE: LifecycleV1 = LifecycleV1 {
    attach,
    detach,
    can_stop,
    settle,
    reload_done,
};

fn provider(refuses: bool) -> Row {
    let parts: &'static [PartRef] = Box::leak(Box::new([PartRef::new(
        PART_LIFECYCLE,
        1,
        &LIFECYCLE as *const LifecycleV1 as *const (),
    )]));
    let row: &'static ProviderRow = Box::leak(Box::new(ProviderRow::new(7, parts)));
    let inst = Box::leak(Box::new([refuses as u8; 4])).as_mut_ptr() as *mut c_void;
    // SAFETY: the row and its tables are leaked; the extent is the whole address space.
    Row::Provider(unsafe { ProviderHandle::resolve(row, 0..usize::MAX, inst) }.unwrap())
}

#[test]
fn a_refused_attach_detaches_only_the_providers_that_attached() {
    let rows: &'static [Row] = Box::leak(
        Vec::from([
            provider(false),
            provider(false),
            provider(true),
            provider(false),
        ])
        .into_boxed_slice(),
    );
    let table = ServiceTable {
        rows,
        failed: 0,
        core: core::ptr::null(),
    };
    let c = ApplyCtx {
        owner: 1,
        reg: core::ptr::null(),
        base: 0,
    };
    ATTACHED.store(0, Ordering::SeqCst);
    DETACHED.store(0, Ordering::SeqCst);
    assert_eq!(unsafe { table.attach_providers(&c) }, E_MISSING);
    // three attach calls (the third refused, the fourth never ran); the two that attached are detached, and no more
    assert_eq!(ATTACHED.load(Ordering::SeqCst), 3);
    assert_eq!(DETACHED.load(Ordering::SeqCst), 2);
}
