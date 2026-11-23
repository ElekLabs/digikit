//! A table with fake functions in two groups: the package facade calls the entry of its own group and function.
use super::{real, TABLE};
use crate::package::resident as facade;
use crate::test_lock::LOCK;
use dkrt_abi::resident::{ArenaFns, CardFns, Group, ResidentTable};

extern "C" fn fake_writable() -> bool {
    false
}
unsafe extern "C" fn fake_alloc(n: u32) -> u32 {
    0x1000 + n
}
unsafe extern "C" fn fake_free(addr: u32) -> bool {
    addr == 0x1010
}
unsafe extern "C" fn fake_stats(out: *mut u32) {
    *out = 11;
    *out.add(1) = 22;
}
extern "C" fn fake_size() -> u32 {
    333
}
extern "C" fn fake_base() -> u32 {
    0x4000
}

static FAKE_CARD: Group<CardFns> = Group::new(CardFns { writable: fake_writable });
static FAKE_ARENA: Group<ArenaFns> = Group::new(ArenaFns { alloc: fake_alloc, free: fake_free, stats: fake_stats, size: fake_size, base: fake_base });

#[test]
fn the_facade_calls_the_entry_of_its_group() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    // the real table with two groups swapped for fakes (leaked: the package keeps `'static` tables)
    let mut t: ResidentTable = unsafe { core::ptr::read(&TABLE) };
    t.card = &FAKE_CARD;
    t.arena = &FAKE_ARENA;
    let t: &'static ResidentTable = std::boxed::Box::leak(std::boxed::Box::new(t));
    unsafe {
        facade::install(t).expect("the fake table fits");
        assert!(!facade::card::writable());
        assert_eq!(facade::arena::alloc(16).get(), 0x1010);
        assert!(facade::arena::free(dkrt_abi::dev_addr::DevAddr::new(0x1010)));
        assert!(!facade::arena::free(dkrt_abi::dev_addr::DevAddr::new(0x1020)));
        assert_eq!(facade::arena::stats(), (11, 22));
        assert_eq!(facade::arena::size(), 333);
        assert_eq!(facade::arena::base(), 0x4000);

        facade::install(&TABLE).expect("the real table fits");
    }
    let _ = real();
}

/// A refused table changes nothing: the package keeps the table it had.
#[test]
fn a_refused_table_leaves_the_installed_one() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let mut t: ResidentTable = unsafe { core::ptr::read(&TABLE) };
    t.header.major += 1;
    let t: &'static ResidentTable = std::boxed::Box::leak(std::boxed::Box::new(t));
    unsafe {
        assert!(facade::install(t).is_err());
        assert_eq!(facade::card::writable(), crate::card::writable());
    }
}

/// The core logs the reason of a refusal and installs no hooks: it runs without the package.
#[test]
fn the_core_logs_a_refusal_and_installs_no_hooks() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let mut t: ResidentTable = unsafe { core::ptr::read(&TABLE) };
    t.header.major += 1;
    let t: &'static ResidentTable = std::boxed::Box::leak(std::boxed::Box::new(t));
    crate::hooks::without_package(|| unsafe {
        let next = crate::platform::st().log_next;
        crate::hooks::attach_package(t);
        assert!(!crate::hooks::installed());
        let s = crate::platform::st();
        assert_eq!(s.log_next, next + 1);
        let e = &s.log[(next as usize) % dkrt_abi::runtime_state::LOG_ENTRIES];
        assert!(e.msg.starts_with(b"resident: major"));
        assert_eq!(e.value, (dkrt_abi::resident::RESIDENT_MAJOR + 1) as u32);
    });
}
