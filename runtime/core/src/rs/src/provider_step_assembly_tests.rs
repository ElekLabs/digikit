//! The owner's two-phase call over the fixture `local-step` (the service whose key hook runs `Owner::step`): through the fixture's own entries as the runtime calls them. A step
//! enters the owner once, its `act` phase calls the service's own export (what a firmware function that calls back into the service does) and finds the owner idle; one deliberate
//! nested call inside the owner is refused and counted, so a count of 0 means something. The device test (`runtime/core/tests/services/owner-step.device.test.ts`) reads the same
//! words from the running file.
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_fixture_local_step::{
    dk_local_step_key, dk_svc_start, dk_svc_stop, step_once, ARM, DK_SVC_OWNER, LOCAL_STEP_STATS,
};

fn words() -> [u32; 3] {
    // SAFETY: the test is the only task
    unsafe { core::array::from_fn(|i| read_volatile(addr_of!(LOCAL_STEP_STATS[i]))) }
}

#[test]
fn a_step_counts_one_entry_and_its_act_finds_the_owner_idle() {
    dk_svc_stop();
    let _instance = dk_svc_start().expect("the instance of local-step starts");
    // SAFETY: the test is the only task
    unsafe { write_volatile(addr_of_mut!(LOCAL_STEP_STATS), [0; 3]) };

    // the act phase called the export (42), which entered the owner again
    assert_eq!(step_once(), Some(42));
    dk_local_step_key();
    assert_eq!(words(), [2, 2, 0], "steps equal re-entries");
    assert_eq!(DK_SVC_OWNER.refused(), 0, "no call found the owner busy");

    // one nested call inside the owner (armed through the file's word): refused, counted once, and the step still runs
    // SAFETY: as above
    unsafe { write_volatile(addr_of_mut!(LOCAL_STEP_STATS[ARM]), 1) };
    assert_eq!(step_once(), Some(42));
    assert_eq!(words(), [3, 3, 0], "the arming word is cleared");
    assert_eq!(DK_SVC_OWNER.refused(), 1, "the nested call was refused");
    step_once();
    assert_eq!(DK_SVC_OWNER.refused(), 1, "later steps are not refused");

    dk_svc_stop();
    assert_eq!(step_once(), None, "no owner, no step");
}
