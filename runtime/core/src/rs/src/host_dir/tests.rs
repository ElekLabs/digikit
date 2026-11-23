use super::*;
use crate::test_lock::LOCK;
use dkrt_sdk::{HOST_CORE_OWNED, HOST_COUNT, HOST_PACKAGE_OWNED, HOST_DSP, HOST_PACKAGE_LO, HOST_PROVIDERS};

fn entry(id: u32) -> Option<u32> {
    unsafe { (*core::ptr::addr_of!(DKRT_HOST_DIR)).get(id) }
}

fn ready() -> bool {
    unsafe { (*core::ptr::addr_of!(DKRT_HOST_DIR)).is_ready() }
}

#[test]
fn a_package_publishes_its_own_ids_only_and_clear_empties_all() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        clear();
        assert!(publish_package(HOST_DSP, 0x4000_1230));
        assert_eq!((entry(HOST_DSP), entry(HOST_PROVIDERS)), (Some(0x4000_1230), None));
        assert!(!publish_package(HOST_COUNT as u32, 4), "an id this core does not know");
        assert!(!publish_package(HOST_PACKAGE_LO, 4), "a core-owned id is not the package's to write");
        assert_eq!(entry(HOST_PACKAGE_LO), None);
        complete();
        assert!(ready());
        clear();
        assert_eq!((entry(HOST_DSP), ready()), (None, false));
    }
}

/// The core fills exactly the ids it owns (the generated list, so an id appended with `owner = "core"` and no value here fails this), and none of the package's.
#[test]
fn the_core_fills_every_id_it_owns_and_none_of_the_packages() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        clear();
        publish_core(0x4010_0000, 0x4011_0000);
        for &id in HOST_CORE_OWNED.iter() {
            assert!(entry(id).is_some(), "core-owned id {id} is filled");
        }
        for &id in HOST_PACKAGE_OWNED.iter() {
            assert_eq!(entry(id), None, "package-owned id {id} stays the package's");
        }
        assert_eq!((entry(HOST_PACKAGE_LO), ready()), (Some(0x4010_0000), false), "filling is not completing");
        clear();
    }
}
