//! The commit: the services told in three passes and the records they write.
use super::*;

#[test]
fn the_services_are_told_before_init_and_commit_in_three_passes_after_it() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        use dkrt_abi::service::commit_phase::{DECLARE, PREPARE, WRITE};
        let mut l = loader(Init::Ok);
        assert_eq!(l.load(PATH.as_ptr()), OK);
        // the module is the first block of the arena: its base is FIRST_ADDR, its image 32 bytes
        assert_eq!(l.r.svc, vec![Svc::Begin(0), Svc::Commit(0, PREPARE, FIRST_ADDR, 32), Svc::Commit(0, WRITE, FIRST_ADDR, 32), Svc::Commit(0, DECLARE, FIRST_ADDR, 32)]);
        assert_eq!(*ORDER.lock().unwrap(), ["begin", "init", "commit", "commit", "commit"]);
        // a service that wrote no record has no block of its own: the module block stays alone
        assert_eq!(l.m.live.iter().map(|b| b.0).collect::<Vec<_>>(), vec![FIRST_ADDR]);
    }
}

#[test]
fn a_service_that_refuses_the_commit_undoes_the_load_for_every_service() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        use dkrt_abi::service::commit_phase::{PREPARE, WRITE};
        // in the first pass: nothing was written, no second pass
        let mut l = loader(Init::Ok);
        l.r.service.refuse = Some((PREPARE, E_TYPE_USED));
        assert_eq!(l.load(PATH.as_ptr()), E_TYPE_USED);
        assert!(l.m.live.is_empty());
        assert_eq!(st().modules[0].used, 0);
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)], "the registry takes everything of the record down");
        assert!(!l.r.svc.iter().any(|e| matches!(e, Svc::Commit(_, WRITE, ..))));
        // in the second pass, with the record block allocated: the block is freed too
        let mut l = loader(Init::Ok);
        l.r.service = FakeService { record: 16, skew: 0, refuse: Some((WRITE, E_PARAM_FULL)) };
        assert_eq!(l.load(PATH.as_ptr()), E_PARAM_FULL);
        assert!(l.m.live.is_empty(), "the module block and the record block are free");
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
        assert_eq!(st().loads, 0);
        // the record can be used again
        l.r.service = FakeService { record: 0, skew: 0, refuse: None };
        assert_eq!(l.load(PATH.as_ptr()), OK);
        assert_eq!(st().modules[0].used, 1);
    }
}

#[test]
fn the_records_a_service_writes_are_one_block_that_the_walker_checks() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        // a record of a kind no service owns: the block was allocated, written and refused by the walker, and everything is given back
        let mut l = loader(Init::Ok);
        l.r.service = FakeService { record: 16, skew: 0, refuse: None };
        let r = l.load(PATH.as_ptr());
        assert_ne!(r, OK);
        assert!(l.m.live.is_empty());
        assert_eq!(l.m.log.iter().filter(|e| matches!(e, Ev::Alloc(..))).count(), 3, "module block, collect block, record block");
        assert_eq!(l.r.log, vec![Ev::Add(0), Ev::Remove(0)]);
        // a service that writes more in the second pass than it measured is refused before the walker runs
        let mut l = loader(Init::Ok);
        l.r.service = FakeService { record: 16, skew: 8, refuse: None };
        assert_eq!(l.load(PATH.as_ptr()), E_FORMAT);
        assert!(l.m.live.is_empty());
    }
}
