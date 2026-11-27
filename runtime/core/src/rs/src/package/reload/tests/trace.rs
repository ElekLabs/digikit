//! The lifecycle events of the live job: a begin and an end for each step it passes, a note for each busy answer, one operation number for the whole job.
use super::*;
use crate::dlog::{self, Ev};
use crate::package::lifecycle::stem_hash;
use crate::test_lock::LOCK;
use dkrt_lifecycle::{Phase, Step};

static mut SEEN: std::vec::Vec<Ev> = std::vec::Vec::new();
const PATH: &[u8] = b"mods/job-trace.dkm\0";

unsafe extern "C" fn keep(e: *const Ev) {
    // the other tests of this module run jobs too: only the events that name this test's file count
    if (*e).kind == logk::LIFECYCLE && (*e).a[1] == stem_hash(PATH.as_ptr()) {
        (*core::ptr::addr_of_mut!(SEEN)).push(*e);
    }
}

/// The (phase, step, status) of each event of one job over `s`, with the operation numbers it carried.
fn events_of(s: &mut Script, op: u32) -> (std::vec::Vec<(u32, u32, i32)>, std::vec::Vec<u32>) {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        (*core::ptr::addr_of_mut!(SEEN)).clear();
        dlog::dk_debug_log_sink_set(keep as *const () as usize as *mut u8, core::ptr::null_mut());
        let mut j = Job::new();
        assert_eq!(j.queue(s, op, PATH.as_ptr()), OK);
        let mut n = 0;
        while j.phase == 1 && n < 100_000 {
            j.poll(s);
            if let Some(t) = s.now.as_mut() {
                *t += 100;
            }
            n += 1;
        }
        dlog::test_reset_sinks();
        let seen = &*core::ptr::addr_of!(SEEN);
        (seen.iter().map(|e| (e.a[2] & 0xff, e.a[2] >> 8 & 0xff, e.a[3] as i32)).collect(), seen.iter().map(|e| e.a[0]).collect())
    }
}

fn ev(phase: Phase, step: Step, status: i32) -> (u32, u32, i32) {
    (phase as u32, step as u32, status)
}

#[test]
fn a_load_has_a_begin_and_an_end_for_its_steps_a_note_for_a_busy_answer_and_one_operation_number() {
    let mut s = Script::new();
    s.found = None;
    s.load_busy = 1;
    let (got, ops) = events_of(&mut s, LIVE_LOAD);
    let want = [
        ev(Phase::Begin, Step::JobCheck, 0),
        ev(Phase::End, Step::JobCheck, 0),
        ev(Phase::Begin, Step::JobLoad, 0),
        ev(Phase::Note, Step::BusyRetry, E_BUSY),
        ev(Phase::End, Step::JobLoad, 0),
        ev(Phase::Note, Step::JobDone, OK),
    ];
    assert_eq!(got, want);
    assert!(ops.iter().all(|o| *o == ops[0]), "{ops:?}");
}

#[test]
fn a_refused_job_ends_its_check_with_the_refusal() {
    let mut s = Script::new();
    s.found = None;
    s.check = E_FORMAT;
    let (got, _) = events_of(&mut s, LIVE_LOAD);
    assert_eq!(got, [ev(Phase::Begin, Step::JobCheck, 0), ev(Phase::End, Step::JobCheck, E_FORMAT), ev(Phase::Note, Step::JobDone, E_FORMAT)]);
}

#[test]
fn a_reload_passes_quiesce_unload_and_load_in_order() {
    let mut s = Script::new();
    let (got, _) = events_of(&mut s, LIVE_RELOAD);
    let begins: std::vec::Vec<u32> = got.iter().filter(|e| e.0 == Phase::Begin as u32).map(|e| e.1).collect();
    assert_eq!(begins, [Step::JobCheck, Step::JobQuiesce, Step::JobUnload, Step::JobLoad].map(|s| s as u32));
}
