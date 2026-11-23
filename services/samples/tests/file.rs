//! The service file's own code over the owner cell, on the host: the `dk_sample_pool_*` exports and the two pool event slots reach the owner through `DK_SVC_OWNER.with`, and
//! answer "no service" (`E_CONTEXT`, the event dropped) when there is none. The imports of the file (`dkc_*`) are stubs here.
use dkrt_abi::dsp::Job;
use dkrt_abi::provider::DspPart;
use dkrt_abi::service::Claim;
use dkrt_sdk::{E_CONTEXT, E_CONTRACT, E_NOT_FOUND, E_SEL_PATH};
use dkrt_svc_rt::ImportCore;
use dkrt_svc_samples::table::POOL_TABLE;
use std::sync::Mutex;

// the service file's source, as the ColdFire build has it: its exports reach the owner as `crate::DK_SVC_OWNER`
#[path = "../dks/src/lib.rs"]
pub mod file;
use file::glue::{
    dk_sample_pool_add, dk_sample_pool_clear, dk_sample_pool_open, dk_sample_pool_path,
    dk_sample_pool_set, dk_sample_pool_status, pool_done_fill, pool_lost_fill,
};
use file::{dk_svc_start, dk_svc_stop};
pub use file::{glue, DK_SVC_OWNER};

// the core imports (`dkc_*`) of the service file, as host functions (this test links no runtime)
#[path = "../../../runtime/core/test-support/stubs/core_imports.rs"]
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

/// Module record 4 gets the client slot 0 (handle 1): its track-table record in the DSP section, as the loader hands it to `claim`.
fn claim_a_table(started: bool) {
    assert!(started);
    let mut bank = [0u8; 32];
    for (i, w) in [2u32, 1, 0x100, 0x200].iter().enumerate() {
        bank[i * 4..i * 4 + 4].copy_from_slice(&w.to_le_bytes());
    }
    let mut job = Job::empty();
    job.bank = bank.as_ptr();
    let mut c = Claim {
        rec: 4,
        job: &job,
        base: 0x1000,
        byte_shift: 0,
        state_at: 0,
    };
    DK_SVC_OWNER
        .with(|s| DspPart::<ImportCore>::claim(s, &ImportCore, &mut c))
        .expect("the owner is live");
}

#[test]
fn the_pool_calls_answer_at_once_while_the_owner_is_inside_a_call() {
    let _t = TURN.lock().unwrap_or_else(|e| e.into_inner());
    dk_svc_start().expect("start");
    // a host build has no sample-pool adapter: say there is one, then publish the client
    POOL_TABLE.with(&ImportCore, |t| t.reset(true));
    claim_a_table(true);
    let h = 1 as *mut u8;
    // the UI task is inside the owner (a tick, a peak window) when the MIDI task calls: the owner's guard refuses every call into the object, the table answers
    let seen = DK_SVC_OWNER.run(None, |_| {
        // SAFETY: NUL-terminated literals; the buffers are locals
        unsafe {
            let mut buf = [0u8; 32];
            let r = (
                dk_sample_pool_add(h, c"SND/A".as_ptr().cast()),
                dk_sample_pool_add(h, c"SND/B".as_ptr().cast()),
                dk_sample_pool_add(h, c"SND/A".as_ptr().cast()),
                dk_sample_pool_status(h, 0),
                dk_sample_pool_set(h, 3, c"SND/C".as_ptr().cast()),
                dk_sample_pool_clear(h, 0),
                dk_sample_pool_status(h, 0),
                dk_sample_pool_add(h, c"SND/D".as_ptr().cast()),
                dk_sample_pool_path(h, 3, buf.as_mut_ptr(), 32),
            );
            assert_eq!(&buf[..5], b"SND/C");
            // the guard still refuses the nested call into the object itself
            assert!(DK_SVC_OWNER.with(|_| ()).is_none());
            Some(r)
        }
    });
    // QUEUED is 1; the cleared entry is the hole the next add takes
    assert_eq!(seen, Some((0, 1, 0, 1, 0, 0, 0, 0, 5)));
    assert_eq!(
        DK_SVC_OWNER.refused(),
        1,
        "only the nested `with` of the check was refused"
    );
    dk_svc_stop();
    // SAFETY: as above
    assert_eq!(
        unsafe { dk_sample_pool_status(h, 0) },
        E_CONTEXT,
        "no service"
    );
}
