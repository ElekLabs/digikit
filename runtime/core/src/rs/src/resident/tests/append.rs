//! A producer built against a later layout (an entry appended to a group, several groups grown, a table that grew): the package accepts it, reads its own entries
//! at their own offsets and copies exactly the bytes of its own layout. A group that is too small or missing is refused, group by group.
use super::{open, real, TABLE};
use dkrt_abi::resident::{Group, Reason, Resident, ResidentTable, RESIDENT_MINOR};

/// A group as a producer with `E` appended to its `T` builds it: the same words, a larger size and the extra entry after the last known one.
#[repr(C)]
struct Grown<T, E> {
    size: u32,
    entries: T,
    extra: E,
}

/// The bytes the producer's extra entry holds: no entry of the package has this value, so a copy that reaches it shows.
const EXTRA: u8 = 0xa5;

/// The real group `g` grown by `E` filled with `EXTRA` (leaked: a table keeps `'static` groups).
fn grown<T, E: Copy>(g: *const Group<T>, extra: E) -> *const Group<T> {
    let mut out = core::mem::MaybeUninit::<Grown<T, E>>::zeroed();
    // SAFETY: `Group<T>` is the prefix of `Grown<T, E>` (both `repr(C)`: the size word, then the entries); the extra entry is written after it.
    unsafe {
        let p = out.as_mut_ptr();
        core::ptr::copy_nonoverlapping(g as *const u8, p as *mut u8, core::mem::size_of::<Group<T>>());
        (*p).size = (core::mem::size_of::<T>() + core::mem::size_of::<E>()) as u32;
        core::ptr::addr_of_mut!((*p).extra).write(extra);
        std::boxed::Box::leak(std::boxed::Box::new(out.assume_init())) as *const Grown<T, E> as *const Group<T>
    }
}

/// The real group `g` with a size word of `size_of::<T>() - 1`: one byte short of the package's layout.
fn short<T>(g: *const Group<T>) -> (*const Group<T>, u32) {
    let need = core::mem::size_of::<T>();
    let mut out = core::mem::MaybeUninit::<Group<T>>::zeroed();
    // SAFETY: a bitwise copy of a group of function pointers is a group; the size word is its first four bytes.
    unsafe {
        core::ptr::copy_nonoverlapping(g as *const u8, out.as_mut_ptr() as *mut u8, core::mem::size_of::<Group<T>>());
        (out.as_mut_ptr() as *mut u32).write(need as u32 - 1);
        (std::boxed::Box::leak(std::boxed::Box::new(out.assume_init())) as *const Group<T>, need as u32 - 1)
    }
}

/// A copy of the real table's words, to change.
fn copy() -> ResidentTable {
    // SAFETY: the table holds integers and pointers to statics; a bitwise copy is a table.
    unsafe { core::ptr::read(&TABLE) }
}

/// A `Resident` and the zeroed bytes after it: a copy that is too long writes into them.
#[repr(C)]
struct Guarded {
    resident: core::mem::MaybeUninit<Resident>,
    guard: [u8; 64],
}

/// The bytes of the groups `table` gives for the package, and whether the guard after them is untouched.
fn opened(table: &ResidentTable) -> (std::vec::Vec<u8>, bool) {
    let mut g: Guarded = unsafe { core::mem::zeroed() };
    unsafe { dkrt_abi::resident::check(table, RESIDENT_MINOR, g.resident.as_mut_ptr()) }.expect("the grown table is accepted");
    let bytes = unsafe { core::slice::from_raw_parts(g.resident.as_ptr() as *const u8, core::mem::size_of::<Resident>()) }.to_vec();
    (bytes, g.guard.iter().all(|b| *b == 0))
}

/// The bytes of the real groups.
fn expected() -> std::vec::Vec<u8> {
    let r = real();
    unsafe { core::slice::from_raw_parts(&r as *const Resident as *const u8, core::mem::size_of::<Resident>()) }.to_vec()
}

/// The fields of the table, in one list for the tests that go through every group.
macro_rules! each_group {
    ($m:ident) => {
        $m!(views, platform, boot, dlog, store, store_writer, card, settings, arena, selection, worker, io, intro, sdk_files, api, fault, transport, state, imports, links, host_dir);
    };
}

/// The package reads its own entries from a producer that has one more entry in a group: the same `Resident` as from the real table, and no byte after it written.
/// Each group takes the appended entry in turn (the last group's copy would run past the `Resident` when its length were the producer's).
#[test]
fn a_group_with_an_appended_entry_is_read_at_the_package_layout() {
    let want = expected();
    macro_rules! one {
        ($($f:ident),*) => {$({
            let mut t = copy();
            t.$f = grown(t.$f, [EXTRA; 12]);
            let (got, guard_ok) = opened(&t);
            assert_eq!(got, want, "{} grown", stringify!($f));
            assert!(guard_ok, "{} grown: bytes after the groups written", stringify!($f));
        })*};
    }
    each_group!(one);
}

/// Several groups grown, by different amounts, and a table that grew a group pointer: still accepted and read at the package's offsets.
#[test]
fn a_table_with_several_grown_groups_is_read_at_the_package_layout() {
    let mut t = copy();
    t.header.minor = RESIDENT_MINOR + 2;
    t.header.size += 8;
    t.views = grown(t.views, [EXTRA; 4]);
    t.boot = grown(t.boot, [EXTRA; 8]);
    t.store = grown(t.store, [EXTRA; 16]);
    t.api = grown(t.api, [EXTRA; 4]);
    t.imports = grown(t.imports, [EXTRA; 40]);
    let (got, guard_ok) = opened(&t);
    assert_eq!(got, expected());
    assert!(guard_ok);
    // a package built against the minor before the growth reads the same
    assert!(open(&t, RESIDENT_MINOR + 1).is_ok());
    assert_eq!(open(&t, RESIDENT_MINOR + 3).err().map(|r| r.reason), Some(Reason::Minor));
}

/// A group one byte short of the package's layout is refused with its name and both sizes, for every group (the first and the last of the table included), and nothing
/// is copied.
#[test]
fn a_short_group_is_refused_by_name() {
    macro_rules! one {
        ($($f:ident),*) => {$({
            let mut t = copy();
            let (g, found) = short(t.$f);
            t.$f = g;
            let mut guarded: Guarded = unsafe { core::mem::zeroed() };
            let r = unsafe { dkrt_abi::resident::check(&t, RESIDENT_MINOR, guarded.resident.as_mut_ptr()) }.err().expect("a short group is refused");
            assert_eq!((r.reason, r.group, r.found), (Reason::GroupSize, stringify!($f), found), "{}", stringify!($f));
            assert_eq!(r.need, found + 1);
            let untouched = unsafe { core::slice::from_raw_parts(&guarded as *const Guarded as *const u8, core::mem::size_of::<Guarded>()) };
            assert!(untouched.iter().all(|b| *b == 0), "{}: refused, yet copied", stringify!($f));
        })*};
    }
    each_group!(one);
}

/// A null group pointer is refused with the group's name, for every group.
#[test]
fn a_missing_group_is_refused_by_name() {
    macro_rules! one {
        ($($f:ident),*) => {$({
            let mut t = copy();
            t.$f = core::ptr::null();
            let r = open(&t, RESIDENT_MINOR).err().expect("a missing group is refused");
            assert_eq!((r.reason, r.group, r.found, r.need), (Reason::GroupMissing, stringify!($f), 0, 0), "{}", stringify!($f));
        })*};
    }
    each_group!(one);
}

/// `PackageHooks` is the table the package gives the core, not the resident table, and its appends follow the same rule: the group of table minor 3 (`ui`) is the last one, right after
/// the group of the minor before (`provider`), so a package built before it is a prefix of the table the core reads, and the core reads `ui` only when the package's minor has it
/// (`UI_HOOKS_SINCE`, `hooks::has_group`).
#[test]
fn the_ui_hooks_are_the_last_group_of_the_package_hooks() {
    use core::mem::{offset_of, size_of};
    use dkrt_abi::hooks::{PackageHooks, ProviderHooks, UiHooks, UI_HOOKS_SINCE, UI_RECORD_NAME_SINCE};
    assert_eq!(offset_of!(PackageHooks, provider) + size_of::<ProviderHooks>(), offset_of!(PackageHooks, ui));
    assert_eq!(offset_of!(PackageHooks, ui) + size_of::<UiHooks>(), size_of::<PackageHooks>());
    assert_eq!(UI_HOOKS_SINCE, 3, "the group came with table minor 3 (RC2s-3)");
    assert!(RESIDENT_MINOR >= UI_HOOKS_SINCE);
    // minor 4 appended `record_name` after `settings_rows` inside that group, so a minor-3 package's `ui` group is a prefix of the one the core reads
    assert_eq!(UI_RECORD_NAME_SINCE, 4, "the entry came with table minor 4 (E1 step 1)");
    assert_eq!(offset_of!(UiHooks, settings_rows) + size_of::<Option<unsafe extern "C" fn(*mut u8)>>(), offset_of!(UiHooks, record_name));
    assert_eq!(offset_of!(UiHooks, record_name) + size_of::<Option<unsafe extern "C" fn(u32, *mut u8) -> i32>>(), size_of::<UiHooks>());
}

/// `links` is the group that version 1.1 appended (RC2s-2 step c), `host_dir` the one of 1.2 (the host directory) and `exports` the one of 1.3 (the menu's calls, RC2s-3): the
/// layout of an older package is the prefix of a newer table and of the checked `Resident`, so a 1.0, 1.1 or 1.2 package reads a 1.3 table at the offsets it always had, and a
/// newer package finds an older core out by its minor (and by the pointer the table is too short for).
#[test]
fn the_appended_groups_are_the_last_ones_so_an_older_package_is_a_prefix() {
    use core::mem::{offset_of, size_of};
    assert_eq!(RESIDENT_MINOR, 4, "this test is about the appends of 1.1 to 1.3; minor 4 appended only an entry to the `ui` group of `PackageHooks` (above): a later minor that appends to this table after `exports` needs its own case");
    let pointer = size_of::<*const ()>();
    assert_eq!(offset_of!(ResidentTable, links) + pointer, offset_of!(ResidentTable, host_dir));
    assert_eq!(offset_of!(ResidentTable, host_dir) + pointer, offset_of!(ResidentTable, exports));
    assert_eq!(offset_of!(ResidentTable, exports) + pointer, size_of::<ResidentTable>());
    assert_eq!(offset_of!(Resident, links) + size_of::<dkrt_abi::resident::LinkRows>(), offset_of!(Resident, host_dir));
    assert_eq!(offset_of!(Resident, host_dir) + size_of::<dkrt_abi::resident::HostDirFns>(), offset_of!(Resident, exports));
    assert_eq!(offset_of!(Resident, exports) + size_of::<dkrt_abi::resident::ExportFns>(), size_of::<Resident>());
    // the 1.2 table, as the 1.2 core built it: no `exports` pointer, minor 2
    let mut core_1_2 = copy();
    core_1_2.header.minor = 2;
    core_1_2.header.size = offset_of!(ResidentTable, exports) as u32;
    core_1_2.exports = core::ptr::null();
    let refused = open(&core_1_2, 3).err().expect("a 1.3 package on a 1.2 core is refused");
    assert_eq!((refused.reason, refused.found, refused.need), (Reason::Minor, 2, 3));
    // the 1.1 table, as the 1.1 core built it: no `host_dir` pointer, minor 1
    let mut core_1_1 = copy();
    core_1_1.header.minor = 1;
    core_1_1.header.size = offset_of!(ResidentTable, host_dir) as u32;
    core_1_1.host_dir = core::ptr::null();
    core_1_1.exports = core::ptr::null();
    let refused = open(&core_1_1, 2).err().expect("a 1.2 package on a 1.1 core is refused");
    assert_eq!((refused.reason, refused.found, refused.need), (Reason::Minor, 1, 2));
    // the 1.0 table, as the 1.0 core built it: no `links` and no `host_dir`, minor 0
    let mut core_1_0 = copy();
    core_1_0.header.minor = 0;
    core_1_0.header.size = offset_of!(ResidentTable, links) as u32;
    core_1_0.links = core::ptr::null();
    core_1_0.host_dir = core::ptr::null();
    core_1_0.exports = core::ptr::null();
    let refused = open(&core_1_0, 1).err().expect("a 1.1 package on a 1.0 core is refused");
    assert_eq!((refused.reason, refused.found, refused.need), (Reason::Minor, 0, 1));
    // an older package on the newest table: accepted (the version rule); it reads the groups it knows as they were
    assert!(open(&copy(), 1).is_ok());
    assert!(open(&copy(), 0).is_ok());
    assert_eq!(copy().header.minor, RESIDENT_MINOR);
}

/// `PackageState.package_bytes` was appended to a shared view (RC2s-3): a view layout change is a major bump (`resident.rs`, "Shared views"), so the major is 2. A package built
/// for major 2 is refused by a core of major 1 (which has the shorter `PackageState`) and by a core of a later major, with the version reason: no package reads a view
/// laid out for another major.
#[test]
fn a_change_of_a_shared_view_layout_is_a_major_version() {
    assert_eq!(dkrt_abi::resident::RESIDENT_MAJOR, 2, "package_bytes was appended to PackageState: the major is 2");
    let mut previous_core = copy();
    previous_core.header.major = 1;
    let refused = open(&previous_core, RESIDENT_MINOR).err().expect("a major-2 package on a major-1 core is refused");
    assert_eq!((refused.reason, refused.found, refused.need), (Reason::Major, 1, 2));
    let mut newer_core = copy();
    newer_core.header.major = 3;
    assert_eq!(open(&newer_core, RESIDENT_MINOR).err().map(|r| r.reason), Some(Reason::Major));
}
