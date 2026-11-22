//! A live unload.
use super::*;

#[test]
fn an_unload_checks_the_module_drains_tears_down_and_ends_not_loaded_with_the_services_told_once() {
    let mut s = Script::new();
    s.quiesce_busy = 3;
    s.unload_busy = 2;
    assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, OK, step::DONE));
    // the refusals of a module (menus, services), no file check, no phase, no load half
    assert!(first(&s, "find") < first(&s, "refusal") && first(&s, "refusal") < first(&s, "quiesce"));
    assert!(first(&s, "quiesce") < first(&s, "unload") && first(&s, "unload") < first(&s, "unloaded") && first(&s, "unloaded") < first(&s, "finish ok"));
    assert!(first(&s, "finish ok") < first(&s, "phase off"));
    for never in ["alloc", "refusal load", "check", "phase on", "begin load", "load", "on_loaded", "dsp", "retire", "abort", "finish torn"] {
        assert!(!s.log.contains(&never), "{never}: {:?}", s.log);
    }
    assert!(s.torn_seen, "the module is gone");
    assert_eq!(s.log.iter().filter(|l| **l == "finish ok").count(), 1);
    assert_eq!(s.log.iter().filter(|l| **l == "unloaded").count(), 1);
    assert_eq!(s.log.iter().filter(|l| **l == "quiesce").count(), 4, "a busy drain is asked again on the next pass");
}

#[test]
fn an_unload_that_is_refused_changes_nothing_and_one_of_a_module_that_is_gone_is_not_found() {
    for refusal in [E_RELOAD_PLAYING, E_RELOAD_MENUS, E_RELOAD_ROUTED, E_CARD_BUSY, dkrt_sdk::proto::E_STATE, E_SAFE_MODE] {
        let mut s = Script::new();
        s.refusal = Some(refusal);
        assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, refusal, step::DONE), "{refusal}");
        nothing_changed(&s, &format!("{refusal}"));
    }
    let mut s = Script::new();
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_UNLOAD, b"mods/a.dkm\0".as_ptr()), OK);
        s.found = None;
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_NOT_FOUND));
    nothing_changed(&s, "gone");
}

#[test]
fn an_unload_whose_drain_or_teardown_does_not_end_gives_the_module_back() {
    // the drain: nothing was freed
    let mut s = Script::new();
    s.quiesce_busy = u32::MAX;
    assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, E_BUSY, step::DONE));
    assert!(s.log.contains(&"abort") && !s.log.contains(&"unload") && !s.log.contains(&"unloaded"));
    assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
    assert!(!s.phase_on && !s.torn_seen);
    // the teardown stops before `fini`
    let mut s = Script::new();
    s.unload_busy = u32::MAX;
    assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, E_BUSY, step::DONE));
    assert!(s.log.contains(&"abort") && !s.log.contains(&"unloaded"));
    assert!(!s.torn_seen, "before fini the module is live");
    // the DSP rows cannot be cleared: retained, said at once
    let mut s = Script::new();
    s.unload_busy = u32::MAX;
    let mut j = Job::new();
    unsafe {
        assert_eq!(j.queue(&mut s, LIVE_UNLOAD, b"mods/a.dkm\0".as_ptr()), OK);
        for _ in 0..4 {
            j.poll(&mut s);
        }
        s.retained = true;
        j.poll(&mut s);
    }
    assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
    assert!(!j.torn && !s.log.contains(&"unloaded"));
}
