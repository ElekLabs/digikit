//! The service file's own code over the owner cell, on the host: the `dk_sample_pool_*` exports and the two pool event slots reach the owner through `DK_SVC_OWNER.with`, and
//! answer "no service" (`E_CONTEXT`, the event dropped) when there is none. The imports of the file (`dkc_*`) are stubs here.
use dkrt_sdk::{E_CONTEXT, E_CONTRACT, E_NOT_FOUND, E_SEL_PATH};
use std::sync::Mutex;

// the service file's source, as the ColdFire build has it: its exports reach the owner as `crate::DK_SVC_OWNER`
#[path = "../dks/src/lib.rs"]
pub mod file;
use file::glue::{
    dk_sample_pool_add, dk_sample_pool_open, dk_sample_pool_status, pool_done_fill, pool_lost_fill,
};
use file::{dk_svc_start, dk_svc_stop};
pub use file::{glue, DK_SVC_OWNER};

mod stubs;
use stubs::RUNNING;

/// The cell is one per process: the tests take turns.
static TURN: Mutex<()> = Mutex::new(());

#[test]
fn without_an_owner_the_exports_answer_no_service_and_the_events_are_dropped() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    assert!(DK_SVC_OWNER.with(|_| ()).is_none());
    // SAFETY: a null path is refused before it is read; the events touch no module code
    unsafe {
        assert_eq!(
            dk_sample_pool_add(1 as *mut u8, core::ptr::null()),
            E_CONTEXT
        );
        assert_eq!(dk_sample_pool_status(1 as *mut u8, 0), E_CONTEXT);
        pool_done_fill(1, 0);
        pool_lost_fill();
    }
}

#[test]
fn the_exports_reach_the_started_owner_and_a_stopped_one_is_gone() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    dk_svc_start().expect("start");
    // SAFETY: as above; `out` is a local
    unsafe {
        // the owner answers, not the fallback: no client holds the handle, a null path is refused first
        assert_eq!(dk_sample_pool_status(1 as *mut u8, 0), E_NOT_FOUND);
        assert_eq!(
            dk_sample_pool_add(1 as *mut u8, core::ptr::null()),
            E_SEL_PATH
        );
        let mut out: *mut u8 = core::ptr::null_mut();
        assert_eq!(
            dk_sample_pool_open(core::ptr::null_mut(), &mut out),
            E_CONTRACT
        );
        RUNNING.store(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            dk_sample_pool_open(core::ptr::null_mut(), &mut out),
            E_NOT_FOUND,
            "the module has no table"
        );
        RUNNING.store(0, std::sync::atomic::Ordering::SeqCst);
        pool_done_fill(1, 0);
        pool_lost_fill();
    }
    dk_svc_stop();
    // SAFETY: as above
    assert_eq!(unsafe { dk_sample_pool_status(1 as *mut u8, 0) }, E_CONTEXT);
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

#[test]
fn the_end_of_a_peak_job_inside_a_call_of_the_service_is_queued_and_applied_after_it() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    dk_svc_start().expect("start");
    // the job task's end entry fires while the UI task is inside the owner: the end waits, it is not refused and not applied beside the call
    let ctx = &file::glue::DK_PEAK_JOBS as *const _ as *mut u8;
    let seen = DK_SVC_OWNER.run(None, |_| {
        // SAFETY: the file's own entry, with the cell it was posted with
        unsafe { file::glue::peak_done(ctx, 0) };
        Some(DK_SVC_OWNER.refused())
    });
    assert_eq!(seen, Some(0), "queued, not refused");
    assert_eq!(DK_SVC_OWNER.refused(), 0);
    dk_svc_stop();
}
