extern crate std;

use super::*;
use crate::provider::{PartRef, ProviderRow};
use core::sync::atomic::{AtomicU32, Ordering};
use std::boxed::Box;
use std::vec::Vec;

/// The lifecycle functions of a hand-built table: each adds a distinct bit to `CALLS` and the instance's first byte says which instance was used.
static CALLS: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn attach(i: *mut c_void, _: *const c_void, c: *const ApplyCtx) -> i32 {
    CALLS.fetch_or(
        1 | (*(i as *mut u8) as u32) << 8 | ((*c).owner as u32) << 16,
        Ordering::SeqCst,
    );
    7
}
unsafe extern "C" fn detach(_: *mut c_void, _: *const c_void, owner: u8) {
    CALLS.fetch_or(2 | (owner as u32) << 16, Ordering::SeqCst);
}
unsafe extern "C" fn can_stop(_: *mut c_void, _: *const c_void) -> i32 {
    -3
}
unsafe extern "C" fn settle(_: *mut c_void, _: *const c_void, _: *const SettleCtx) {}
unsafe extern "C" fn reload_done(_: *mut c_void, _: *const c_void) {}

fn lifecycle() -> &'static LifecycleV1 {
    Box::leak(Box::new(LifecycleV1 {
        attach,
        detach,
        can_stop,
        settle,
        reload_done,
    }))
}

unsafe extern "C" fn begin(_: *mut c_void, _: *const c_void, _: usize) {}
unsafe extern "C" fn commit(
    _: *mut c_void,
    _: *const c_void,
    _: usize,
    _: *const ModuleCommitCtx,
) -> i32 {
    0
}
unsafe extern "C" fn check_step(
    _: *mut c_void,
    _: *const c_void,
    step: u32,
    _: *const CheckCtx,
) -> i32 {
    -(step as i32)
}
/// Returns the body length: the handle passes the length its kinds table gives, less the 8-byte header.
static APPLIED: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn apply(
    _: *mut c_void,
    _: *const c_void,
    _: u32,
    _: *const u8,
    len: u32,
    _: *const ApplyCtx,
) -> i32 {
    APPLIED.fetch_add(1, Ordering::SeqCst);
    len as i32
}

static KINDS: [(u32, u32); 2] = [(30, 24), (31, 8)];

fn records() -> &'static RecordsV1 {
    Box::leak(Box::new(RecordsV1 {
        kinds: KINDS.as_ptr(),
        nkinds: 2,
        begin,
        commit,
        check: check_step,
        apply,
    }))
}

fn part<T>(tag: u32, version: u16, table: &'static T) -> PartRef {
    PartRef::new(tag, version, table as *const T as *const ())
}

fn row(parts: Vec<PartRef>) -> &'static ProviderRow {
    let parts: &'static [PartRef] = Box::leak(parts.into_boxed_slice());
    Box::leak(Box::new(ProviderRow::new(0xabcd, parts)))
}

fn check_ctx() -> CheckCtx {
    CheckCtx {
        reg: core::ptr::null(),
        image_size: 0,
    }
}

fn no_records() -> ApplyCtx {
    ApplyCtx {
        owner: 1,
        reg: core::ptr::null(),
        base: 0,
    }
}

fn whole() -> Range<usize> {
    0..usize::MAX
}

fn instance(first: u8) -> *mut c_void {
    Box::leak(Box::new([first; 4])).as_mut_ptr() as *mut c_void
}

fn resolve(r: &'static ProviderRow, inst: *mut c_void) -> Result<ProviderHandle, Status> {
    // SAFETY: the host test's row and tables are leaked boxes.
    unsafe { ProviderHandle::resolve(r, whole(), inst) }
}

#[test]
fn a_part_that_was_not_adopted_answers_with_the_old_default() {
    let h = resolve(
        row(std::vec![part(PART_LIFECYCLE, 1, lifecycle())]),
        instance(0),
    )
    .unwrap();
    let core = core::ptr::null();
    unsafe {
        assert_eq!(h.name(), 0xabcd);
        assert!(h.kinds().is_empty());
        h.module_begin(core, 3);
        assert_eq!(h.check(core, 4, &check_ctx()), OK);
        assert_eq!(
            h.apply_record(core, 30, core::ptr::null(), &no_records()),
            OK
        );
        assert!(h.dsp_clear(core, 0));
        assert_eq!(h.cap(core, 1), None);
        assert_eq!(h.table_check(core, 0, &mut TableFault::default()), 0);
        assert_eq!(h.attribute(core, 1, 2), None);
        assert_eq!(h.query(1, &[], &mut []), dkrt_sdk::E_MISSING);
        let mut want = 0;
        assert!(h.dsp_row_step(core, 0, &mut want, true).is_none());
    }
}

#[test]
fn the_lifecycle_functions_get_the_instance_and_the_consumer() {
    let inst = instance(5);
    let h = resolve(row(std::vec![part(PART_LIFECYCLE, 1, lifecycle())]), inst).unwrap();
    assert_eq!(h.instance(), inst);
    let ctx = ApplyCtx {
        owner: 9,
        reg: core::ptr::null(),
        base: 0x4000,
    };
    CALLS.store(0, Ordering::SeqCst);
    unsafe {
        assert_eq!(h.attach(core::ptr::null(), &ctx), 7);
        assert_eq!(h.can_stop(core::ptr::null()), -3);
    }
    assert_eq!(CALLS.load(Ordering::SeqCst), 1 | 5 << 8 | 9 << 16);
    unsafe { h.detach(core::ptr::null(), 4) };
    assert_eq!(CALLS.load(Ordering::SeqCst) & 2, 2);
}

#[test]
fn a_record_of_a_kind_the_service_owns_gets_its_body_length_and_another_kind_is_not_passed() {
    let h = resolve(
        row(std::vec![
            part(PART_LIFECYCLE, 1, lifecycle()),
            part(PART_RECORDS, 1, records()),
        ]),
        instance(0),
    )
    .unwrap();
    let ctx = ApplyCtx {
        owner: 1,
        reg: core::ptr::null(),
        base: 0,
    };
    unsafe {
        assert_eq!(h.kinds(), &KINDS[..]);
        // the body is the record's whole length less its header: 24 - 8 and 8 - 8
        assert_eq!(
            h.apply_record(core::ptr::null(), 30, core::ptr::null(), &ctx),
            16
        );
        assert_eq!(
            h.apply_record(core::ptr::null(), 31, core::ptr::null(), &ctx),
            0
        );
        assert_eq!(APPLIED.load(Ordering::SeqCst), 2);
        // a kind of another service: the part is not called
        assert_eq!(
            h.apply_record(core::ptr::null(), 99, core::ptr::null(), &ctx),
            OK
        );
        assert_eq!(APPLIED.load(Ordering::SeqCst), 2);
        assert_eq!(h.check(core::ptr::null(), 6, &check_ctx()), -6);
    }
}

#[test]
fn a_part_version_newer_than_the_core_knows_is_refused_with_the_abi_status() {
    let table = lifecycle() as *const LifecycleV1 as *const ();
    // the lifecycle part itself, and an optional part
    let newer_lifecycle = row(std::vec![PartRef::new(
        PART_LIFECYCLE,
        LIFECYCLE_NEWEST + 1,
        table
    )]);
    assert_eq!(resolve(newer_lifecycle, instance(0)).err(), Some(E_SVC_ABI));
    let newer_records = row(std::vec![
        part(PART_LIFECYCLE, 1, lifecycle()),
        PartRef::new(
            PART_RECORDS,
            RECORDS_NEWEST + 1,
            records() as *const _ as *const ()
        ),
    ]);
    assert_eq!(resolve(newer_records, instance(0)).err(), Some(E_SVC_ABI));
    // the newest version the core knows is accepted
    let newest = row(std::vec![
        part(PART_LIFECYCLE, LIFECYCLE_NEWEST, lifecycle()),
        part(PART_RECORDS, RECORDS_NEWEST, records()),
    ]);
    assert!(resolve(newest, instance(0)).is_ok());
}

#[test]
fn a_table_outside_the_package_a_null_instance_and_a_misaligned_table_are_refused() {
    let r = row(std::vec![part(PART_LIFECYCLE, 1, lifecycle())]);
    assert_eq!(resolve(r, core::ptr::null_mut()).err(), Some(E_FORMAT));
    // an extent that holds the row and its part list but not the table
    let at = r as *const ProviderRow as usize;
    let extent = at..at + size_of::<ProviderRow>();
    let h = unsafe { ProviderHandle::resolve(r, extent, instance(0)) };
    assert_eq!(h.err(), Some(E_FORMAT));
    let odd = row(std::vec![PartRef::new(
        PART_LIFECYCLE,
        1,
        (lifecycle() as *const LifecycleV1 as usize + 1) as *const ()
    )]);
    assert_eq!(resolve(odd, instance(0)).err(), Some(E_FORMAT));
}

/// A row, its part list, the lifecycle and records tables and the kinds array in one buffer, like a package: the extent of the buffer holds every table the row names,
/// so no test depends on where statics and heap blocks lie.
#[repr(C)]
struct Pack {
    row: ProviderRow,
    parts: [PartRef; 2],
    lc: LifecycleV1,
    records: RecordsV1,
    kinds: [(u32, u32); 2],
}

fn pack(kinds: *const (u32, u32), nkinds: u32) -> &'static mut Pack {
    let p = Box::leak(Box::new(Pack {
        row: ProviderRow::new(0xabcd, &[]),
        parts: [part(PART_LIFECYCLE, 1, lifecycle()); 2],
        lc: LifecycleV1 {
            attach,
            detach,
            can_stop,
            settle,
            reload_done,
        },
        records: RecordsV1 {
            kinds,
            nkinds,
            begin,
            commit,
            check: check_step,
            apply,
        },
        kinds: [(30, 24), (31, 8)],
    }));
    p.parts[0] = PartRef::new(PART_LIFECYCLE, 1, &p.lc as *const LifecycleV1 as *const ());
    p.parts[1] = PartRef::new(PART_RECORDS, 1, &p.records as *const RecordsV1 as *const ());
    // SAFETY: the buffer is leaked and its part list is never moved.
    p.row = ProviderRow::new(0xabcd, unsafe { &*(&p.parts[..] as *const [PartRef]) });
    p
}

#[test]
fn a_kinds_array_that_leaves_the_package_or_overflows_is_refused() {
    let resolve_in = |p: &Pack, e: Range<usize>| unsafe {
        ProviderHandle::resolve(&p.row, e, instance(0)).err()
    };
    let whole_pack = |p: &Pack| {
        let a = p as *const Pack as usize;
        a..a + size_of::<Pack>()
    };
    let widen = whole_pack;
    let ok = pack(core::ptr::null(), 0);
    ok.records.kinds = ok.kinds.as_ptr();
    ok.records.nkinds = 2;
    assert_eq!(resolve_in(ok, widen(ok)), None);
    // more kinds than the package holds
    let long = pack(core::ptr::null(), 0);
    long.records.kinds = long.kinds.as_ptr();
    long.records.nkinds = 3;
    assert_eq!(resolve_in(long, widen(long)), Some(E_FORMAT));
    // a kinds array outside the package
    let out = pack(KINDS.as_ptr(), 2);
    assert_eq!(resolve_in(out, widen(out)), Some(E_FORMAT));
    // a length that overflows the address space
    let big = pack(core::ptr::null(), 0);
    big.records.kinds = big.kinds.as_ptr();
    big.records.nkinds = u32::MAX;
    assert_eq!(resolve_in(big, widen(big)), Some(E_FORMAT));
    // a misaligned array
    let odd = pack(core::ptr::null(), 0);
    odd.records.kinds = (odd.kinds.as_ptr() as usize + 1) as *const (u32, u32);
    odd.records.nkinds = 1;
    assert_eq!(resolve_in(odd, widen(odd)), Some(E_FORMAT));
}

/// The state functions of a hand-built table: the instance's first byte is the exported byte, and an import records what it was given.
static IMPORTED: AtomicU32 = AtomicU32::new(0);
unsafe extern "C" fn state_export_max(_: *mut c_void, _: *const c_void) -> u32 {
    4
}
unsafe extern "C" fn state_export(i: *mut c_void, _: *const c_void, out: *mut u8, cap: u32) -> u32 {
    *out = *(i as *mut u8);
    cap + 10
}
unsafe extern "C" fn state_import(
    _: *mut c_void,
    _: *const c_void,
    from: *const u8,
    len: u32,
    has: bool,
) -> i32 {
    // 0x100 marks "None", else the first byte and the length
    let v = if has {
        (*from as u32) | len << 16
    } else {
        0x100
    };
    IMPORTED.store(v, Ordering::SeqCst);
    0
}

fn state(version: u16, accepts_from: u16) -> &'static StateV1 {
    Box::leak(Box::new(StateV1 {
        version,
        accepts_from,
        export_max: state_export_max,
        export: state_export,
        import: state_import,
    }))
}

fn with_state(version: u16, accepts_from: u16) -> ProviderHandle {
    resolve(
        row(std::vec![
            part(PART_LIFECYCLE, 1, lifecycle()),
            part(PART_STATE, 1, state(version, accepts_from)),
        ]),
        instance(9),
    )
    .unwrap()
}

#[test]
fn a_service_without_the_state_part_exports_nothing_and_takes_anything() {
    let h = resolve(
        row(std::vec![part(PART_LIFECYCLE, 1, lifecycle())]),
        instance(0),
    )
    .unwrap();
    let core = core::ptr::null();
    assert_eq!(h.state_versions(), None);
    assert!(!h.state_accepts(1));
    unsafe {
        assert_eq!(h.state_export_max(core), 0);
        assert_eq!(h.state_export(core, &mut [0u8; 4]), 0);
        assert_eq!(h.state_import(core, Some((1, &[1]))), OK);
    }
}

#[test]
fn old_bytes_reach_import_only_inside_the_accepted_version_range() {
    // this copy writes version 3 and takes 2 to 3
    let h = with_state(3, 2);
    let core = core::ptr::null();
    assert_eq!(h.state_versions(), Some((3, 2)));
    for (old, taken) in [(1, false), (2, true), (3, true), (4, false)] {
        assert_eq!(h.state_accepts(old), taken, "old version {old}");
        unsafe { h.state_import(core, Some((old, &[7, 8]))) };
        let want = if taken { 7 | 2 << 16 } else { 0x100 };
        assert_eq!(IMPORTED.load(Ordering::SeqCst), want, "old version {old}");
    }
    // no bytes at all: the service decides with None
    unsafe { h.state_import(core, None) };
    assert_eq!(IMPORTED.load(Ordering::SeqCst), 0x100);
}

#[test]
fn export_is_clamped_to_the_buffer_the_runtime_gave() {
    let h = with_state(1, 1);
    let mut out = [0u8; 4];
    unsafe {
        assert_eq!(h.state_export_max(core::ptr::null()), 4);
        // the table's function claims cap + 10; the handle returns what fits
        assert_eq!(h.state_export(core::ptr::null(), &mut out), 4);
    }
    assert_eq!(out[0], 9);
}

#[test]
fn a_state_part_version_newer_than_the_core_knows_is_refused_with_the_abi_status() {
    let r = row(std::vec![
        part(PART_LIFECYCLE, 1, lifecycle()),
        PartRef::new(
            PART_STATE,
            STATE_NEWEST + 1,
            state(1, 1) as *const _ as *const ()
        ),
    ]);
    assert_eq!(resolve(r, instance(0)).err(), Some(E_SVC_ABI));
}
