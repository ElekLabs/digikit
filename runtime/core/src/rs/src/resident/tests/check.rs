//! `dkrt_abi::resident::check`: the table of this build is accepted and each way a table can not fit is refused with its reason.
use super::{open, real, TABLE};
use dkrt_abi::resident::{check, CardFns, Group, Reason, ResidentTable, RESIDENT_MAGIC, RESIDENT_MAJOR, RESIDENT_MINOR};

/// A copy of the real table's words, to change.
fn copy() -> ResidentTable {
    // SAFETY: the table holds integers and pointers to statics; a bitwise copy is a table.
    unsafe { core::ptr::read(&TABLE) }
}

/// The reason, the value found, the value needed and the group of the refusal.
fn refuse(t: &ResidentTable, built_minor: u16) -> (Reason, u32, u32, &'static str) {
    let r = open(t, built_minor).err().expect("the table is refused");
    (r.reason, r.found, r.need, r.group)
}

#[test]
fn the_table_of_this_build_is_accepted() {
    let _ = real();
}

/// A newer producer (minor ahead of the package's) and an equal one are accepted; an older one is not.
#[test]
fn a_minor_at_least_the_built_one_is_accepted() {
    let mut t = copy();
    t.header.minor = RESIDENT_MINOR + 3;
    assert!(open(&t, RESIDENT_MINOR).is_ok());
    assert_eq!(refuse(&copy(), RESIDENT_MINOR + 1), (Reason::Minor, RESIDENT_MINOR as u32, RESIDENT_MINOR as u32 + 1, ""));
}

#[test]
fn a_table_of_another_kind_is_refused() {
    let mut t = copy();
    t.header.magic = RESIDENT_MAGIC ^ 1;
    assert_eq!(refuse(&t, RESIDENT_MINOR), (Reason::Magic, RESIDENT_MAGIC ^ 1, RESIDENT_MAGIC, ""));
    let mut t = copy();
    t.header.major = RESIDENT_MAJOR + 1;
    assert_eq!(refuse(&t, RESIDENT_MINOR), (Reason::Major, RESIDENT_MAJOR as u32 + 1, RESIDENT_MAJOR as u32, ""));
    let mut into = core::mem::MaybeUninit::<dkrt_abi::resident::Resident>::uninit();
    assert_eq!(unsafe { check(core::ptr::null(), RESIDENT_MINOR, into.as_mut_ptr()) }.err().map(|r| r.reason), Some(Reason::NoTable));
}

/// A producer built against an older layout: fewer group pointers, a null one, or a group with fewer bytes than the package uses.
#[test]
fn a_table_or_group_that_is_too_small_is_refused() {
    let mut t = copy();
    t.header.size -= 4;
    assert_eq!(refuse(&t, RESIDENT_MINOR), (Reason::TableSize, t.header.size, core::mem::size_of::<ResidentTable>() as u32, ""));

    let mut t = copy();
    t.card = core::ptr::null();
    assert_eq!(refuse(&t, RESIDENT_MINOR), (Reason::GroupMissing, 0, 0, "card"));

    let empty = Group::new(());
    let mut t = copy();
    t.card = &empty as *const Group<()> as *const Group<CardFns>;
    assert_eq!(refuse(&t, RESIDENT_MINOR), (Reason::GroupSize, 0, core::mem::size_of::<CardFns>() as u32, "card"));
}

/// The reason is what the boot log shows: a text of at most 23 bytes and a NUL.
#[test]
fn every_reason_fits_a_log_entry() {
    let all = [Reason::NoTable, Reason::Magic, Reason::Major, Reason::Minor, Reason::TableSize, Reason::GroupMissing, Reason::GroupSize];
    let mut seen = std::vec::Vec::new();
    for reason in all {
        let m = dkrt_abi::resident::Refusal { reason, found: 0, need: 0, group: "" }.message();
        assert!(m.len() <= 24 && m.last() == Some(&0), "{reason:?}");
        assert!(!seen.contains(&m), "{reason:?} has its own text");
        seen.push(m);
    }
}
