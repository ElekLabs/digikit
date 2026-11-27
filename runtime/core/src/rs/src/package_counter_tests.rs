//! The boot counter and the system package file: writing `digikit/system.dksp` through the store and the RETRY command clear the counter that keeps boots
//! core-only (`level::Level::keeps_counter`), over the real store and writer on the model of the card of `store_tests`.
use crate::admin_cmds::cmd_retry;
use crate::storage::sdk_files as sf;
use crate::selection::{current_with, update_with, Choice};
use crate::host_io::card;
use crate::store_tests::{begin, in_step, pattern, rename, sdk_put, slot, upload, Fixture, STORE};
use crate::platform::PLATFORM;
use crate::transport::DKRT_UPLOAD_LAST;
use core::ptr::{addr_of, read_volatile};
use dkrt_sdk::proto::{E_INUSE, E_STATE};
use dkrt_sdk::sel::Record;
use dkrt_sdk::OK;

/// A record with `counter` boots counted on the card.
fn card_with_counter(counter: u32) {
    let written = unsafe {
        match current_with(&PLATFORM) {
            Choice::Use { mut record, copy, .. } => {
                record.boot_counter = counter;
                update_with(&PLATFORM, record, copy, true)
            }
            _ => {
                let mut r = Record::empty();
                r.boot_counter = counter;
                update_with(&PLATFORM, r, 0, false)
            }
        }
    };
    assert_eq!(written, OK, "the record is written");
}

fn counter() -> u32 {
    match unsafe { current_with(&PLATFORM) } {
        Choice::Use { record, .. } => record.boot_counter,
        _ => panic!("the card holds a record"),
    }
}

fn retry() -> i32 {
    unsafe {
        cmd_retry(1);
        read_volatile(addr_of!(DKRT_UPLOAD_LAST))
    }
}

#[test]
fn writing_the_system_package_file_clears_the_boot_counter_and_other_files_do_not() {
    let _f = Fixture::new();
    card_with_counter(5);
    assert_eq!(upload("modules/a.dkm", &pattern(3000, 3)), OK);
    assert_eq!(counter(), 5, "another file leaves the counter");
    assert_eq!(upload("digikit/system.dksp.old", &pattern(600, 5)), OK);
    assert_eq!(counter(), 5, "a path that only starts like the package's leaves it too");
    assert_eq!(upload("digikit/system.dksp", &pattern(5000, 7)), OK);
    assert_eq!(counter(), 0, "a new package file gets its chance");
}

#[test]
fn retry_clears_the_boot_counter_without_an_upload_and_waits_for_an_open_one() {
    let _f = Fixture::new();
    card_with_counter(6);
    assert_eq!(retry(), OK);
    assert_eq!(counter(), 0);
    card_with_counter(6);
    let bytes = pattern(1000, 3);
    assert_eq!(begin("modules/b.dkm", bytes.len(), slot(&bytes)), OK);
    assert_eq!(retry(), E_STATE, "an upload is open");
    assert_eq!(counter(), 6);
}

#[test]
fn a_rename_into_the_package_path_clears_the_counter_and_one_away_from_it_does_not() {
    let _f = Fixture::new();
    assert_eq!(upload("digikit/new.dksp", &pattern(3000, 3)), OK);
    card_with_counter(5);
    assert_eq!(rename("digikit/new.dksp", "digikit/system.dksp"), OK);
    assert_eq!(counter(), 0, "the file now at the package path is a new package");
    card_with_counter(5);
    assert_eq!(rename("digikit/system.dksp", "digikit/old.dksp"), OK);
    assert_eq!(counter(), 5, "taking the package away is no new package");
}

#[test]
fn a_commit_of_many_files_that_names_the_package_clears_the_counter() {
    let _f = Fixture::new();
    card_with_counter(5);
    assert_eq!(sdk_put(STORE, "p/a", &pattern(1500, 7), sf::FILE_DEFER), OK);
    assert_eq!(sdk_put(STORE, "p/b", &pattern(700, 3), sf::FILE_DEFER), OK);
    assert_eq!(in_step(|| unsafe { sf::sdk_commit() }), OK);
    assert_eq!(counter(), 5, "two other files leave the counter");
    assert_eq!(sdk_put(STORE, "p/c", &pattern(900, 5), sf::FILE_DEFER), OK);
    assert_eq!(sdk_put(STORE, "digikit/system.dksp", &pattern(1200, 9), sf::FILE_DEFER), OK);
    assert_eq!(in_step(|| unsafe { sf::sdk_commit() }), OK);
    assert_eq!(counter(), 0, "one of the committed files is the package");
}

#[test]
fn retry_on_a_card_the_runtime_may_not_write_is_refused_and_writes_nothing() {
    let _f = Fixture::new();
    // somebody else's data in the area: not blank, not ours
    card(|c| {
        c.sectors.insert(crate::store_tests::L.first + 5, [0x55; 512]);
    });
    unsafe { crate::card::check() };
    assert!(!crate::card::writable());
    let writes = card(|c| c.writes);
    assert_eq!(retry(), E_INUSE);
    assert_eq!(card(|c| c.writes), writes, "nothing reached the card");
}
