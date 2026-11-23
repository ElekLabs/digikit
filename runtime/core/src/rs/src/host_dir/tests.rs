use super::*;
use crate::test_lock::LOCK;
use dkrt_sdk::{HOST_COUNT, HOST_DSP, HOST_PROVIDERS};

fn entry(id: u32) -> Option<u32> {
    unsafe { (*core::ptr::addr_of!(DKRT_HOST_DIR)).get(id) }
}

#[test]
fn publish_sets_one_entry_refuses_an_unknown_id_and_clear_empties_all() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        clear();
        assert!(publish(HOST_DSP, 0x4000_1230));
        assert_eq!((entry(HOST_DSP), entry(HOST_PROVIDERS)), (Some(0x4000_1230), None));
        assert!(!publish(HOST_COUNT as u32, 4), "an id this core does not know");
        clear();
        assert_eq!(entry(HOST_DSP), None);
    }
}
