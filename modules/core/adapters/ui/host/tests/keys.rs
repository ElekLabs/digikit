//! The reverse lookup of the device's key map: panel code of a key event to the logical key.
use dkui_host::session;

extern "C" {
    fn km_lookup(codes: *const u32, n: u32, code: u32) -> u32;
}

fn lookup(codes: &[u32], code: u32) -> u32 {
    let _g = session();
    unsafe { km_lookup(codes.as_ptr(), codes.len() as u32, code) }
}

#[test]
fn a_panel_code_finds_the_key_whose_entry_holds_it() {
    let map = [0, 10, 12, 25, 26];
    assert_eq!(lookup(&map, 10), 1);
    assert_eq!(lookup(&map, 25), 3);
    assert_eq!(lookup(&map, 26), 4);
}

#[test]
fn a_code_without_a_key_and_code_zero_have_no_name() {
    let map = [0, 10, 0, 25];
    assert_eq!(lookup(&map, 99), 0);
    // key 2 has no code on this device (entry 0): code 0 must not name it
    assert_eq!(lookup(&map, 0), 0);
}

#[test]
fn the_first_key_wins_when_two_share_a_code() {
    assert_eq!(lookup(&[0, 7, 7], 7), 1);
}

#[test]
fn an_empty_map_names_nothing() {
    assert_eq!(lookup(&[0], 5), 0);
}
