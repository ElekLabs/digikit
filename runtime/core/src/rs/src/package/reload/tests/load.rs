//! A live load.
use super::*;

#[test]
fn a_load_checks_loads_and_ends_with_the_services_told_once_and_never_drains_or_unloads() {
    let mut s = Script::new();
    assert_eq!(run_op(&mut s, LIVE_LOAD), (2, OK, step::DONE));
    // the index, the refusals (those of a load: no module record), the file checks, then the load; no phase (a load has no rows of its own to leave out), no drain, no teardown
    assert!(first(&s, "find") < first(&s, "alloc") && first(&s, "alloc") < first(&s, "refusal load") && first(&s, "refusal load") < first(&s, "check"));
    assert!(first(&s, "check") < first(&s, "begin load") && first(&s, "begin load") < first(&s, "load") && first(&s, "load") < first(&s, "on_loaded"));
    assert!(first(&s, "on_loaded") < first(&s, "finish ok") && first(&s, "finish ok") < first(&s, "phase off"));
    for never in ["phase on", "refusal", "quiesce", "unload", "unloaded", "abort", "retire", "finish torn"] {
        assert!(!s.log.contains(&never), "{never}: {:?}", s.log);
    }
    assert!(!s.torn_seen, "a load tears nothing down");
    assert_eq!(s.log.iter().filter(|l| **l == "finish ok").count(), 1);
}

#[test]
fn a_load_with_a_dsp_part_waits_for_it_like_a_reload() {
    let mut s = Script::new();
    s.load = LOADING;
    s.dsp = (3, OK);
    assert_eq!(run_op(&mut s, LIVE_LOAD), (2, OK, step::DONE));
    assert_eq!(s.log.iter().filter(|l| **l == "dsp").count(), 4);
    assert!(!s.log.contains(&"on_loaded") && !s.log.contains(&"retire"));
    assert_eq!(s.log.last(), Some(&"phase off"));
}

#[test]
fn a_load_that_does_not_come_up_ends_as_it_began_with_the_index_free() {
    // the load fails
    let mut s = Script::new();
    s.load = E_INIT;
    assert_eq!(run_op(&mut s, LIVE_LOAD), (2, E_INIT, step::DONE));
    assert!(s.log.contains(&"retire") && !s.log.contains(&"on_loaded") && !s.log.contains(&"finish ok"));
    assert!(first(&s, "retire") < first(&s, "finish failed"));
    assert!(!s.torn_seen && !s.phase_on);
    // its DSP part fails, or stalls and is cancelled
    for (dsp, code) in [((1, E_DSP_XFER), E_DSP_XFER), ((u32::MAX, OK), E_DSP_XFER)] {
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = dsp;
        assert_eq!(run_op(&mut s, LIVE_LOAD), (2, code, step::DONE));
        assert!(s.log.contains(&"retire") && !s.log.contains(&"on_loaded"), "{:?}", s.log);
        assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
        assert!(!s.torn_seen && !s.phase_on);
    }
    // a reload that fails after its teardown keeps the module's index (the module is FAILED, not free): no retire
    let mut s = Script::new();
    s.load = E_INIT;
    assert_eq!(run_op(&mut s, LIVE_RELOAD), (2, E_INIT, step::DONE));
    assert!(!s.log.contains(&"retire"));
}

#[test]
fn a_load_that_is_refused_or_fails_its_check_changes_nothing() {
    for (refusal, check, alloc, want) in [
        (Some(E_RELOAD_PLAYING), OK, Ok(5), E_RELOAD_PLAYING),
        (Some(E_SAFE_MODE), OK, Ok(5), E_SAFE_MODE),
        (Some(dkrt_sdk::proto::E_STATE), OK, Ok(5), dkrt_sdk::proto::E_STATE),
        (None, E_MISSING, Ok(5), E_MISSING),
        (None, E_PLAN_KEY, Ok(5), E_PLAN_KEY),
        (None, OK, Err(E_SEL_COUNT), E_SEL_COUNT),
    ] {
        let mut s = Script::new();
        s.refusal = refusal;
        s.check = check;
        s.alloc = alloc;
        assert_eq!(run_op(&mut s, LIVE_LOAD), (2, want, step::DONE), "{want}");
        nothing_changed(&s, &format!("{want}"));
    }
    // a module of that path came while the job waited: E_LOADED, still nothing changed
    let mut s = Script::new();
    let mut j = Job::new();
    s.found = None;
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_LOAD, b"mods/a.dkm\0".as_ptr()), OK);
        s.found = Some((2, 5));
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_LOADED));
    nothing_changed(&s, "E_LOADED");
}

#[test]
fn a_load_into_a_retained_dsp_is_refused_at_the_queue_and_in_a_running_job() {
    let mut s = Script::new();
    s.retained = true;
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_LOAD, b"mods/a.dkm\0".as_ptr()), E_RELOAD_RETAINED);
    }
    assert_eq!(j.phase, 0);
    let mut s = Script::new();
    s.found = None;
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_LOAD, b"mods/a.dkm\0".as_ptr()), OK);
        s.retained = true;
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
    nothing_changed(&s, "retained");
}
