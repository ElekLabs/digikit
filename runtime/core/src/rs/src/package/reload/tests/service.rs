//! The reload of a service: the checks and the drain over its closure, one call that takes the closure down and brings it back, and the ends of the job around it.
use super::*;
use dkrt_abi::MAX_MODULES;

/// A script whose `find` names the first service record (a provider has no selection index).
fn service_script() -> Script {
    let mut s = Script::new();
    s.service = true;
    s.found = Some((MAX_MODULES, usize::MAX));
    s
}

fn changed_nothing(s: &Script, what: &str) {
    nothing_changed(s, what);
    assert!(!s.log.contains(&"reload service"), "{what}: the service was reloaded: {:?}", s.log);
}

#[test]
fn a_service_reload_checks_drains_reloads_the_closure_in_one_call_and_ends_with_the_services_told_once() {
    let mut s = service_script();
    s.quiesce_busy = 2;
    s.service_busy = 3;
    // the job asks the menus on every pass of the UNLOAD step; the owner stops saying yes once the call has its room (`ReloadState::is_called`)
    assert_eq!(run(&mut s), (2, OK, step::DONE));
    let at = |name: &str| first(&s, name);
    assert!(at("find") < at("refusal") && at("refusal") < at("check") && at("check") < at("quiesce") && at("quiesce") < at("reload service"));
    assert!(at("reload service") < at("finish ok") && at("finish ok") < at("phase off"));
    assert_eq!(s.log.iter().filter(|l| **l == "quiesce").count(), 3, "a busy drain is asked again on the next pass");
    assert_eq!(s.log.iter().filter(|l| **l == "reload service").count(), 4, "a busy restore asks the same call again");
    assert_eq!(s.log.iter().filter(|l| **l == "finish ok").count(), 1);
    for gone in ["phase on", "begin load", "load", "unload", "unloaded", "retire", "on_loaded", "abort"] {
        assert!(!s.log.contains(&gone), "a service reload has no `{gone}` step of its own: {:?}", s.log);
    }
    assert!(s.torn_seen, "the closure was taken down and is back: the state says it was reloaded");
}

#[test]
fn a_service_that_is_busy_gets_the_refusals_asked_again_until_it_is_not() {
    let mut s = service_script();
    s.refusal_busy = 3;
    assert_eq!(run(&mut s), (2, OK, step::DONE));
    assert_eq!(s.log.iter().filter(|l| **l == "refusal").count(), 4, "three busy answers, then the one that passes");
    assert_eq!(s.log.iter().filter(|l| **l == "check").count(), 1, "the file is checked once the refusals pass");
    assert!(s.log.contains(&"reload service"));
}

#[test]
fn a_service_that_stays_busy_ends_the_job_busy_with_nothing_changed() {
    let mut s = service_script();
    s.refusal_busy = u32::MAX;
    assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
    changed_nothing(&s, "a service busy until the timeout");
    assert!(s.log.iter().filter(|l| **l == "refusal").count() > 50, "it asked on every pass until the wait ran out");
}

#[test]
fn a_busy_module_refusal_is_not_asked_again() {
    let mut s = Script::new();
    s.refusal_busy = 1;
    assert_eq!(run(&mut s).1, E_BUSY);
    assert_eq!(s.log.iter().filter(|l| **l == "refusal").count(), 1);
}

#[test]
fn a_service_reload_refused_before_the_call_changes_nothing() {
    for (what, refusal, check) in [("a refusal", Some(E_RELOAD_PLAYING), OK), ("a check", None, E_RELOAD_KEYS), ("a drain that does not end", None, OK)] {
        let mut s = service_script();
        s.refusal = refusal;
        s.check = check;
        if what == "a drain that does not end" {
            s.quiesce_busy = u32::MAX;
        }
        let (phase, result, _) = run(&mut s);
        assert_eq!((phase, result), (2, refusal.unwrap_or(if check != OK { check } else { E_BUSY })), "{what}");
        if what == "a drain that does not end" {
            assert!(s.log.contains(&"abort") && !s.log.contains(&"reload service"));
            assert!(!s.torn_seen);
        } else {
            changed_nothing(&s, what);
        }
    }
}

#[test]
fn a_menu_built_for_a_consumer_during_the_drain_gives_the_closure_back() {
    let mut s = service_script();
    s.quiesce_busy = 3;
    s.menu_after = 0; // asked once right before the call, after the drain: a menu was built meanwhile
    assert_eq!(run(&mut s), (2, E_RELOAD_MENUS, step::DONE));
    assert!(s.log.contains(&"abort") && !s.log.contains(&"reload service"));
    assert!(!s.torn_seen);
    assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
}

#[test]
fn a_service_reload_that_fails_after_a_member_went_down_says_the_closure_is_down() {
    let mut s = service_script();
    s.service_result = E_PLAN_FIT;
    s.can_abort = false;
    assert_eq!(run(&mut s), (2, E_PLAN_FIT, step::DONE));
    assert_not_loaded(&s, "a closure that did not come back");
    // refused by the manager before it took anything down: every member gets its tick back and the old service runs on
    let mut s = service_script();
    s.service_result = E_RELOAD_ROUTED;
    s.can_abort = true;
    assert_eq!(run(&mut s), (2, E_RELOAD_ROUTED, step::DONE));
    assert!(s.log.contains(&"abort") && !s.torn_seen);
    assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
    assert!(!s.phase_on);
}

#[test]
fn a_restore_that_never_parks_ends_the_job_busy_and_torn() {
    let mut s = service_script();
    s.service_busy = u32::MAX;
    s.can_abort = false;
    assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
    assert!(s.log.contains(&"abort"));
    assert_not_loaded(&s, "a restore that did not end");
}

#[test]
fn only_a_reload_takes_the_file_of_a_service() {
    let mut s = service_script();
    let mut k = Job::new();
    unsafe {
        for op in [LIVE_LOAD, LIVE_UNLOAD] {
            assert_eq!(k.queue(&mut s, op, b"services/fx.dks\0".as_ptr()), E_FORMAT, "op {op}");
        }
        assert_eq!(k.phase, 0);
        assert_eq!(k.queue(&mut s, LIVE_RELOAD, b"services/fx.dks\0".as_ptr()), OK);
        assert_eq!((k.phase, k.op), (1, LIVE_RELOAD as u8));
    }
}
