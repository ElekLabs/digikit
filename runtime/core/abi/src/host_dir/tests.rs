use super::*;

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
        unsafe { core::slice::from_raw_parts(&d as *const HostDir as *const u32, 2 + HOST_COUNT) };
    assert_eq!(words[0], HOST_DIR_MAGIC);
    assert_eq!(words[1], HOST_COUNT as u32);
    assert_eq!(words[2 + dkrt_sdk::HOST_TABLE_BAD as usize], 0xdead_beef);
    assert_eq!(core::mem::size_of::<HostDir>(), 4 * (2 + HOST_COUNT));
}
