//! The service file's own code over the owner cell, on the host: the tick, the write job's entries, the project scope table and the two exports reach the owner through
//! `DK_SVC_OWNER`, and answer the neutral way when there is none. The imports of the file (`dkc_*`) are stubs here, and so are the SDK calls (`glue/sdk_imports.rs`).
use dkrt_abi::provider::{check, PartLookup, PART_RECORDS, RECORDS_NEWEST};
use dkrt_sdk::proto::{E_SET_NOKEY, E_SET_OFF};
use std::sync::Mutex;

// the service file's source, as the ColdFire build has it: its code reaches the owner as `crate::DK_SVC_OWNER` and the pending record as `crate::hooks`
#[path = "../dks/src/lib.rs"]
pub mod file;
use file::{dk_svc_start, dk_svc_stop};
pub use file::{glue, hooks, DK_SVC_OWNER};

// the core imports (`dkc_*`) of the service file, as host functions (this test links no runtime)
#[path = "../../../runtime/core/test-support/stubs/core_imports.rs"]
mod stubs;

/// The cell is one per process: the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

fn passes() -> Option<u32> {
    DK_SVC_OWNER.with(|p| p.host().passes())
}

#[test]
fn the_file_adopts_the_lifecycle_part_only() {
    // SAFETY: the row is a static of this binary
    let row = unsafe { check(file::dk_svc_row(), 0..usize::MAX) }.expect("a valid row");
    assert!(
        !matches!(
            row.part(PART_RECORDS, RECORDS_NEWEST),
            PartLookup::Found { .. }
        ),
        "the service owns no record kind"
    );
}

#[test]
fn without_an_owner_the_entries_answer_the_neutral_way() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
    // SAFETY: the entries take no pointer the test does not give
    unsafe {
        file::glue::tick(core::ptr::null_mut());
        assert_eq!(file::glue::dk_project_status(), 0);
        assert_eq!(
            file::glue::dk_project_current(core::ptr::null_mut(), core::ptr::null_mut()),
            dkrt_sdk::E_NOT_FOUND
        );
        assert_eq!(
            (file::glue::SCOPE.get)(b"m".as_ptr(), 1, b"k".as_ptr(), 1, core::ptr::null_mut(), 0),
            E_SET_NOKEY
        );
        assert_eq!(
            (file::glue::SCOPE.put)(b"m".as_ptr(), 1, b"k".as_ptr(), 1, b"v".as_ptr(), 1),
            E_SET_OFF
        );
        assert_eq!(
            (file::glue::SCOPE.remove)(b"m".as_ptr(), 1, b"k".as_ptr(), 1),
            E_SET_OFF
        );
        // no owner: the write job's entries do nothing and fail the step
        assert_eq!(
            file::glue::io_step(core::ptr::null_mut()),
            dkrt_sdk::E_NOT_FOUND
        );
    }
}

#[test]
fn the_tick_and_the_exports_reach_the_one_instance_that_start_made() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    dk_svc_start().expect("start");
    assert_eq!(passes(), Some(0));
    // SAFETY: as above
    unsafe {
        file::glue::tick(core::ptr::null_mut());
        file::glue::tick(core::ptr::null_mut());
        assert_eq!(file::glue::dk_project_status(), 0);
    }
    assert_eq!(passes(), Some(2), "both ticks reached the same instance");
    // a second start is refused and leaves the instance as it is
    assert!(dk_svc_start().is_err());
    assert_eq!(passes(), Some(2));
    dk_svc_stop();
    assert_eq!(passes(), None, "after fini there is no instance");
    // SAFETY: as above
    unsafe { file::glue::tick(core::ptr::null_mut()) };
    assert_eq!(passes(), None);
}

#[test]
fn an_export_called_inside_the_owner_answers_neutrally_and_changes_nothing() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    dk_svc_start().expect("start");
    // `with` refuses a nested call. Nothing in `tick` calls an export or the overlay table synchronously (the engine's module settings change only queues a reload), so this
    // is not reached on the device; the test pins that a nested call is refused and leaves the state as it was
    let nested = DK_SVC_OWNER.with(|_| {
        // SAFETY: as above
        unsafe {
            (
                file::glue::dk_project_status(),
                (file::glue::SCOPE.put)(b"m".as_ptr(), 1, b"k".as_ptr(), 1, b"v".as_ptr(), 1),
            )
        }
    });
    assert_eq!(nested, Some((0, E_SET_OFF)));
    assert_eq!(passes(), Some(0));
    dk_svc_stop();
}

#[test]
fn the_row_reaches_the_object_through_the_owner_guard() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    dk_svc_start().expect("start");
    dkrt_test_support::contract::row_is_guarded(
        file::dk_svc_row(),
        &|f| {
            DK_SVC_OWNER.with(|_| f());
        },
        &|| DK_SVC_OWNER.refused(),
    );
    dk_svc_stop();
}
