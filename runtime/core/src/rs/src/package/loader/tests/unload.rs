//! An unload: no acceptance and no index left behind.
use super::*;

#[test]
fn an_unload_leaves_no_acceptance_and_no_index_behind_and_the_same_file_loads_again_at_its_index() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let mut l = live_a_with_b();
        let mut rt = adopt(&mut l, &[0]);
        assert_eq!(rt.live_load_with(1, PATH_B.as_ptr(), &wide()), OK);
        // the unload half of a job (no block clock: the drain is over at once), then its end state
        l.p.clock.set(None);
        assert_eq!(l.reload_quiesce(0), OK);
        assert_eq!(rt.unload(0), OK);
        l.retire_index(0);
        assert_eq!((st().modules[0].used, l.accepted[0], l.sel_of(0)), (0, false, u8::MAX));
        assert_eq!(l.live_at(SEL_MAX), [false, true, false, false, false, false, false, false], "only b is live");
        assert_eq!(crate::package::entry::tick_of(0), 0);
        let mut out = [0u32; 8];
        assert_eq!(l.keys_of(0, dkneeds::KIND_MACHINE, &mut out), 0, "an unloaded module has no keys for a service");
        assert_eq!(l.m.live.len(), 1, "its image block is freed");
        // another module's retire leaves b alone
        l.retire_index(3);
        assert_eq!((st().modules[1].used, l.sel_of(1), l.accepted[1]), (1, 1, true));
        // the same file loads again into the free record, at the same index, and again after another unload
        for round in 0..2 {
            assert_eq!(l.live_check_load(0, PATH.as_ptr(), &wide()), OK, "round {round}");
            assert_eq!(rt.live_load_with(0, PATH.as_ptr(), &wide()), OK, "round {round}");
            assert_eq!((st().modules[0].used, l.sel_of(0), l.accepted[0]), (1, 0, true), "round {round}");
            assert_eq!(l.keys_of(0, dkneeds::KIND_MACHINE, &mut out), 2);
            assert_eq!(rt.unload(0), OK);
            l.retire_index(0);
        }
        assert_eq!(l.m.live.len(), 1);
    }
}
