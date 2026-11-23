use super::*;
use dkrt_sdk::{HOST_CORE_OWNED, HOST_DIR_READY_MAGIC, HOST_PACKAGE_OWNED};

#[test]
fn an_empty_directory_has_no_address_and_a_set_entry_is_found_by_its_id() {
    let mut d = HostDir::empty();
    for id in 0..HOST_COUNT as u32 {
        assert_eq!(d.get(id), None, "id {id} is empty");
    }
    assert!(d.set(dkrt_sdk::HOST_PROVIDERS, 0x4012_3456));
    assert_eq!(d.get(dkrt_sdk::HOST_PROVIDERS), Some(0x4012_3456));
    assert_eq!(d.get(dkrt_sdk::HOST_DSP), None, "another id stays empty");
    d.clear();
    assert_eq!(
        d.get(dkrt_sdk::HOST_PROVIDERS),
        None,
        "clear empties every entry"
    );
}

#[test]
fn an_id_past_the_count_is_refused_by_set_and_unknown_to_get() {
    let mut d = HostDir::empty();
    assert!(!d.set(HOST_COUNT as u32, 4));
    assert_eq!(d.get(HOST_COUNT as u32), None);
    assert_eq!(d.get(u32::MAX), None);
}

#[test]
fn the_layout_is_the_header_and_one_word_per_id() {
    // the tools read the directory as raw words: magic, count, then the addresses
    let mut d = HostDir::empty();
    d.set(dkrt_sdk::HOST_TABLE_BAD, 0xdead_beef);
    let words: &[u32] =
        unsafe { core::slice::from_raw_parts(&d as *const HostDir as *const u32, 3 + HOST_COUNT) };
    assert_eq!(words[0], HOST_DIR_MAGIC);
    assert_eq!(words[1], HOST_COUNT as u32);
    assert_eq!(words[2], 0, "not ready until complete");
    assert_eq!(words[3 + dkrt_sdk::HOST_TABLE_BAD as usize], 0xdead_beef);
    assert_eq!(core::mem::size_of::<HostDir>(), 4 * (3 + HOST_COUNT));
    d.complete();
    let words: &[u32] =
        unsafe { core::slice::from_raw_parts(&d as *const HostDir as *const u32, 3) };
    assert_eq!(words[2], HOST_DIR_READY_MAGIC);
}

#[test]
fn the_directory_is_ready_only_after_complete_and_clear_takes_that_back() {
    let mut d = HostDir::empty();
    assert!(!d.is_ready());
    d.set(dkrt_sdk::HOST_DSP, 0x4000_0100);
    assert!(!d.is_ready(), "entries alone do not make it ready");
    d.complete();
    assert!(d.is_ready());
    d.clear();
    assert!(!d.is_ready());
}

/// Every id has one owner, and the two generated lists partition the ids: an id appended to `[[host_dir]]` with an owner is found in exactly one list.
#[test]
fn every_id_has_exactly_one_owner() {
    for id in 0..HOST_COUNT as u32 {
        assert!(package_owned(id) != core_owned(id), "id {id} has one owner");
    }
    assert_eq!(HOST_PACKAGE_OWNED.len() + HOST_CORE_OWNED.len(), HOST_COUNT);
    assert!(!package_owned(HOST_COUNT as u32) && !core_owned(HOST_COUNT as u32));
}
