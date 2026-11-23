//! `stage1::start` and `accept_table`: the table is asked before the entry is called, and the entry's hooks are what the boot installs.

use super::*;
use crate::fake_platform::address_map::AddressMap;
use crate::platform::st;
use crate::resident::TABLE;
use core::cell::Cell;
use dkrt_abi::hooks::PackageHooks;
use dkrt_abi::resident::{Reason, ResidentTable, RESIDENT_MINOR};

std::thread_local! {
    /// The table the last entry double was called with.
    static SEEN: Cell<usize> = const { Cell::new(0) };
}

// A package entry as `call_at` calls it: twelve words, the first is the table.
extern "C" fn entry_with_hooks(table: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> usize {
    SEEN.with(|s| s.set(table));
    &crate::package::hooks::HOOKS as *const PackageHooks as usize
}

extern "C" fn entry_without_hooks(table: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize, _: usize) -> usize {
    SEEN.with(|s| s.set(table));
    0
}

/// The text of the newest log entry.
fn last_log() -> String {
    let s = unsafe { st() };
    let e = &s.log[(s.log_next.wrapping_sub(1) as usize) % dkrt_abi::runtime_state::LOG_ENTRIES];
    let n = e.msg.iter().position(|b| *b == 0).unwrap_or(e.msg.len());
    String::from_utf8_lossy(&e.msg[..n]).into_owned()
}

#[test]
fn the_entry_is_called_with_the_resident_table_and_its_hooks_are_given_back() {
    let _lock = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let map = AddressMap::new();
    map.with_addresses(|| unsafe {
        let hooks = start(DevAddr::of_fn(entry_with_hooks as usize), &TABLE).expect("an entry that gives hooks");
        assert!(core::ptr::eq(hooks, &crate::package::hooks::HOOKS));
        assert_eq!(SEEN.with(|s| s.get()), &TABLE as *const ResidentTable as usize);
    });
}

#[test]
fn an_entry_that_answers_null_is_logged_and_gives_no_hooks() {
    let _lock = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let map = AddressMap::new();
    map.with_addresses(|| unsafe {
        assert!(start(DevAddr::of_fn(entry_without_hooks as usize), &TABLE).is_none());
        assert_eq!(last_log(), "package: no hooks");
    });
}

#[test]
fn the_table_check_refuses_a_null_table_and_a_newer_package_with_the_reason_in_the_log() {
    let _lock = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let none = accept_table(core::ptr::null(), 0).expect_err("no table");
        assert_eq!(none.reason, Reason::NoTable);
        log_refusal(&none);
        assert_eq!(last_log(), "resident: no table");
        // a package built against a later minor than this core's table
        let newer = accept_table(&TABLE, RESIDENT_MINOR + 1).expect_err("a newer package");
        assert_eq!((newer.reason, newer.found, newer.need), (Reason::Minor, RESIDENT_MINOR as u32, RESIDENT_MINOR as u32 + 1));
        log_refusal(&newer);
        assert_eq!(last_log(), "resident: minor");
        // an older or equal one is accepted
        for minor in 0..=RESIDENT_MINOR {
            accept_table(&TABLE, minor).unwrap_or_else(|e| panic!("minor {minor}: {:?}", e.reason));
        }
    }
}

/// `load_checked` is the order the boot runs: load, ask the table, and on a refusal log it, give the block back and answer `Incompatible`. A table that fits leaves the
/// package placed. 
#[test]
fn load_checked_logs_a_refused_table_and_abandons_the_package_but_keeps_one_that_fits() {
    let _lock = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = super::package(RESIDENT_TABLE_MAJOR, RESIDENT_TABLE_MINOR, &super::body(700));
    let storage = super::card_with(&file);
    let mut buf = std::vec![0u8; 512];
    unsafe {
        // a table that fits: placed, kept
        let mut s = Stage1::new(super::StorageSource::new(&storage), super::RecordingPlacer::default(), &mut buf);
        assert_eq!(s.load_checked(&TABLE), PackageStatus::Ok);
        assert!(s.placed().is_some() && !s.placer().abandoned);
        // a table the check refuses (none): logged with its reason, abandoned, not placed
        let mut s = Stage1::new(super::StorageSource::new(&storage), super::RecordingPlacer::default(), &mut buf);
        assert_eq!(s.load_checked(core::ptr::null()), PackageStatus::Incompatible);
        assert!(s.placed().is_none() && s.placer().abandoned, "the block goes back");
        assert_eq!(last_log(), "resident: no table");
    }
}
