//! A reload: check, drain, teardown, load and the failures between.
use super::*;

#[test]
fn a_reload_checks_drains_unloads_loads_and_ends_with_the_services_told_once() {
    let mut s = Script::new();
    s.quiesce_busy = 3;
    s.unload_busy = 2;
    assert_eq!(run(&mut s), (2, OK, step::DONE));
    let order: Vec<&str> = s.log.clone();
    let first = |name: &str| order.iter().position(|l| *l == name).unwrap();
    // nothing changes before the refusals and the check; the phase is on from there to the end; the services hear the end once, after the load
    assert!(first("find") < first("phase on") && first("phase on") < first("refusal") && first("refusal") < first("check"));
    assert!(first("check") < first("quiesce") && first("quiesce") < first("unload") && first("unload") < first("begin load") && first("begin load") < first("load"));
    assert!(first("load") < first("on_loaded") && first("on_loaded") < first("finish ok") && first("finish ok") < first("phase off"));
    assert_eq!(order.iter().filter(|l| **l == "finish ok").count(), 1);
    assert_eq!(order.iter().filter(|l| **l == "quiesce").count(), 4, "a busy drain is asked again on the next pass");
    assert!(!s.phase_on);
}

#[test]
fn a_module_with_a_dsp_part_waits_for_it_and_gets_on_loaded_from_the_dsp_loader_not_the_job() {
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (3, OK);
    assert_eq!(run(&mut s), (2, OK, step::DONE));
    assert_eq!(s.log.iter().filter(|l| **l == "dsp").count(), 4);
    assert!(!s.log.contains(&"on_loaded"));
    assert_eq!(s.log.last(), Some(&"phase off"));
    // its DSP part fails: the module is not loaded, the services are told, the phase is off
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (1, E_DSP_XFER);
    assert_eq!(run(&mut s), (2, E_DSP_XFER, step::DONE));
    assert_not_loaded(&s, "a failed DSP part");
}

#[test]
fn a_dsp_part_that_does_not_end_is_cancelled_and_the_module_is_not_loaded() {
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (u32::MAX, OK);
    assert_eq!(run(&mut s), (2, E_DSP_XFER, step::DONE));
    assert!(s.log.contains(&"cancel dsp"), "the transfer is taken down before the job ends");
    assert_not_loaded(&s, "a DSP part that timed out");
    // a load that ended while the job looked away is read, not cancelled twice
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (u32::MAX, OK);
    s.cancels = false;
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        let mut n = 0;
        while j.step != step::DSP || n < 3 {
            j.poll(&mut s);
            n += 1;
            if n > 50 {
                break;
            }
        }
        for _ in 0..Job::wait_passes() + 5 {
            j.poll(&mut s);
        }
        assert_eq!(j.phase, 1, "cancel found nothing: the job keeps asking for the result");
        s.dsp = (0, OK);
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, OK));
}

#[test]
fn a_refusal_or_a_failed_check_ends_the_job_before_anything_changes() {
    for (refusal, check, want) in [(Some(E_RELOAD_PLAYING), OK, E_RELOAD_PLAYING), (Some(dkrt_sdk::proto::E_STATE), OK, dkrt_sdk::proto::E_STATE), (None, E_RELOAD_KEYS, E_RELOAD_KEYS)] {
        let mut s = Script::new();
        s.refusal = refusal;
        s.check = check;
        assert_eq!(run(&mut s), (2, want, step::DONE));
        assert!(!s.torn_seen);
        assert!(!s.phase_on, "{want}: the phase is off again");
        for changed in ["quiesce", "unload", "load", "finish ok", "finish failed", "finish torn"] {
            assert!(!s.log.contains(&changed), "{want}: {changed} ran");
        }
    }
    // the module is looked up again at the check: one that went away since the queue is `E_NOT_FOUND`
    let mut s = Script::new();
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        s.found = None;
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_NOT_FOUND));
}

#[test]
fn a_drain_that_does_not_end_aborts_cleanly_and_leaves_the_old_module_live() {
    let mut s = Script::new();
    s.quiesce_busy = u32::MAX;
    assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
    assert!(s.log.contains(&"abort") && !s.log.contains(&"unload") && !s.log.contains(&"load"));
    assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
    assert!(!s.phase_on && !s.torn_seen);
    // without a block clock the timeout counts passes
    let mut s = Script::new();
    s.now = None;
    s.quiesce_busy = u32::MAX;
    assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
    // a block clock that stands still (the DSP stopped) does not hold the wait up either
    let mut s = Script::new();
    s.stalled = true;
    s.quiesce_busy = u32::MAX;
    assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
    assert!(s.log.contains(&"abort") && !s.log.contains(&"unload"));
    assert!(!s.phase_on && !s.torn_seen);
}

#[test]
fn a_teardown_that_does_not_end_gives_the_old_module_back_because_unload_answers_busy_only_before_fini() {
    let mut s = Script::new();
    s.unload_busy = u32::MAX;
    assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
    assert!(s.log.contains(&"abort") && !s.log.contains(&"load"));
    assert!(!s.torn_seen, "before fini the old module is live");
    assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
    assert!(!s.phase_on);
}

/// The drain lasts many passes: a menu built for the module meanwhile (its settings page opened) ends the job before the teardown, for an unload and a reload, and the old module stays.
#[test]
fn a_menu_built_during_the_drain_gives_the_old_module_back() {
    for op in [LIVE_RELOAD, LIVE_UNLOAD] {
        let mut s = Script::new();
        s.quiesce_busy = 3;
        s.menu_after = 2; // not at the first pass of the unload step, then yes
        s.unload_busy = 5;
        assert_eq!(run_op(&mut s, op), (2, E_RELOAD_MENUS, step::DONE), "op {op}");
        assert!(s.log.contains(&"abort") && !s.log.contains(&"load"));
        assert!(!s.torn_seen);
        assert!(!s.phase_on);
        assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
    }
}

#[test]
fn a_retained_dsp_refuses_at_the_queue_and_in_a_running_job_at_once() {
    // at the queue: no job, the code that says to restart
    let mut s = Script::new();
    s.retained = true;
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), E_RELOAD_RETAINED);
        assert_eq!(j.phase, 0);
    }
    // in a job that was queued before the DSP was retained: the check refuses
    let mut s = Script::new();
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        s.retained = true;
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
    assert!(!s.phase_on && !s.log.contains(&"quiesce"));
    // a teardown that fails because the DSP rows cannot be cleared: the DSP is retained, the job does not wait out its budget
    let mut s = Script::new();
    s.unload_busy = u32::MAX;
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        for _ in 0..4 {
            j.poll(&mut s);
        }
        s.retained = true;
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
    assert!(s.log.contains(&"abort") && !j.torn);
}

#[test]
fn a_slow_dsp_load_that_goes_on_is_not_cancelled_and_a_stalled_one_is() {
    // ten rows at 17 passes each, and a long image: far more passes than one budget, but the loader moves all the time
    let mut s = Script::new();
    s.load = LOADING;
    s.progressing = true;
    s.dsp = (10 * 17 * 40, OK);
    assert_eq!(run(&mut s), (2, OK, step::DONE));
    assert!(!s.log.contains(&"cancel dsp"));
    // the loader stands still: cancelled after one budget
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (u32::MAX, OK);
    assert_eq!(run(&mut s), (2, E_DSP_XFER, step::DONE));
    assert!(s.log.contains(&"cancel dsp"));
    // progress that stops half way is cancelled one budget after it stopped
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (u32::MAX, OK);
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        s.progressing = true;
        for _ in 0..3000 {
            j.poll(&mut s);
        }
        assert_eq!(j.phase, 1, "a load that moves is left alone");
        s.progressing = false;
        for _ in 0..Job::wait_passes() + 10 {
            j.poll(&mut s);
        }
    }
    assert_eq!((j.phase, j.result), (2, E_DSP_XFER));
}

#[test]
fn a_load_that_fails_ends_with_its_code_and_the_module_is_not_loaded() {
    let mut s = Script::new();
    s.load = E_INIT;
    assert_eq!(run(&mut s), (2, E_INIT, step::DONE));
    assert_not_loaded(&s, "a failed load");
}

#[test]
fn a_reload_does_not_free_its_index_and_the_state_names_the_operation() {
    let mut s = Script::new();
    assert_eq!(run(&mut s), (2, OK, step::DONE));
    assert!(!s.log.contains(&"unloaded") && !s.log.contains(&"retire") && !s.log.contains(&"alloc"));
    for (op, torn) in [(LIVE_LOAD, false), (LIVE_UNLOAD, true), (LIVE_RELOAD, true)] {
        let mut s = Script::new();
        if op == LIVE_LOAD {
            s.found = None;
        }
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, op, b"mods/a.dkm\0".as_ptr()), OK);
            assert_eq!((j.op as u32, j.phase), (op, 1));
            while j.phase == 1 {
                j.poll(&mut s);
            }
        }
        assert_eq!((j.op as u32, j.torn, j.result), (op, torn, OK), "op {op}");
    }
}

#[test]
fn the_checks_of_a_reload_run_with_its_phase_on_and_those_of_a_load_and_an_unload_without() {
    for (op, phase) in [(LIVE_RELOAD, true), (LIVE_LOAD, false), (LIVE_UNLOAD, false)] {
        let mut s = Script::new();
        assert_eq!(run_op(&mut s, op), (2, OK, step::DONE));
        assert_eq!(s.log.contains(&"phase on"), phase, "op {op}");
    }
}

#[test]
fn a_job_has_a_serial_and_the_end_of_the_last_finished_job_outlives_the_next_job() {
    let mut s = Script::new();
    let mut j = Job::new();
    let finish = |j: &mut Job, s: &mut Script| unsafe {
        while j.phase == 1 {
            j.poll(s);
        }
    };
    unsafe {
        assert_eq!((j.serial, j.done_serial), (0, 0));
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
        assert_eq!(j.serial, 1);
        // a refused queueing is no job
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), E_BUSY);
        assert_eq!(j.serial, 1);
        s.load = E_RELOAD_PLAYING;
        finish(&mut j, &mut s);
        assert_eq!((j.phase, j.result), (2, E_RELOAD_PLAYING));
        // the next job starts: its state replaces the result, the last end is kept by serial
        s.load = OK;
        assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/b.dkm\0".as_ptr()), OK);
        assert_eq!((j.serial, j.phase, j.result), (2, 1, OK));
        assert_eq!((j.done_serial, j.done_result), (1, E_RELOAD_PLAYING));
    }
}
