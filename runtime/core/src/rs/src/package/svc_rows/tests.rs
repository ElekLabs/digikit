use super::*;
use crate::package::core_api::{RealCore, REAL_CORE};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{
    name_hash, LifecycleV1, PartRef, Provider, Records, RecordsV1, TickV1, Ticker, LIFECYCLE_NEWEST, PART_LIFECYCLE, PART_RECORDS, PART_TICK,
    RECORDS_NEWEST, TICK_NEWEST,
};
use dkrt_abi::service::Service;
use dkrt_sdk::E_SVC_ABI;
use std::sync::Mutex;
use std::vec::Vec;

struct Nothing;
impl<C: CoreApi> Service<C> for Nothing {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
}

/// An old row named `$name`.
macro_rules! old {
    ($name:literal) => {{
        static R: ServiceRow = ServiceRow::of::<Nothing, RealCore>($name, &[]);
        RowSource::Service(&R)
    }};
}

/// The same as an in-crate row of the table (before the loaded ones).
macro_rules! fixed {
    ($name:literal) => {{
        static R: ServiceRow = ServiceRow::of::<Nothing, RealCore>($name, &[]);
        Row::Service(&R)
    }};
}

/// The name hashes of the first `n` rows of `t`, in table order.
fn order(t: &[Row], n: usize) -> Vec<u32> {
    t[..n].iter().map(|r| unsafe { r.name_hash() }).collect()
}

fn hashes(names: &[&str]) -> Vec<u32> {
    names.iter().map(|n| name_hash(n)).collect()
}

fn empty() -> Row {
    Row::Service(core::ptr::null())
}

#[test]
fn rows_with_the_same_order_use_the_name_hash_and_removal_closes_the_gap() {
    let mut t = [empty(); 1 + PROVIDER_MAX];
    t[0] = fixed!("a");
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 1, 3, old!("d"), None), OK);
        assert_eq!(r.add(&mut t, 1, 1, old!("b"), None), OK);
        assert_eq!(r.add(&mut t, 1, 2, old!("e"), None), OK);
    }
    // the in-crate row stays first; the loaded ones follow by the hash of their names
    let mut by_hash = ["b", "d", "e"];
    by_hash.sort_by_key(|n| name_hash(n));
    assert_eq!(order(&t, 1 + r.len()), hashes(&["a", by_hash[0], by_hash[1], by_hash[2]]));
    // taking one out closes the gap and clears the end; a second remove does nothing
    r.remove(&mut t, 1, 3);
    assert_eq!(r.len(), 2);
    assert!(!order(&t, 1 + r.len()).contains(&name_hash("d")));
    assert!(matches!(t[1 + r.len()], Row::Service(p) if p.is_null()));
    r.remove(&mut t, 1, 3);
    r.remove(&mut t, 1, 0);
    assert_eq!(r.len(), 2);
}

#[test]
fn the_explicit_maximum_precedes_an_unordered_service() {
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 0, 0, old!("a"), None), OK);
        assert_eq!(r.add(&mut t, 0, 1, old!("z"), Some(u32::MAX)), OK);
    }
    assert_eq!(order(&t, r.len()), hashes(&["z", "a"]));
}

#[test]
fn the_six_shipped_rows_keep_their_order_for_every_load_order() {
    fn row(i: usize) -> RowSource<'static> {
        match i {
            0 => old!("samples"),
            1 => old!("machines"),
            2 => old!("fx"),
            3 => old!("lfo"),
            4 => old!("shortcuts"),
            _ => old!("project"),
        }
    }

    fn check(load_order: &mut [usize; 6], at: usize) {
        if at != load_order.len() {
            for i in at..load_order.len() {
                load_order.swap(at, i);
                check(load_order, at + 1);
                load_order.swap(at, i);
            }
            return;
        }
        let mut t = [empty(); PROVIDER_MAX];
        let mut r = Rows::new();
        for &i in load_order.iter() {
            // Alternate old packages and freshly packed ones.
            let order = if i % 2 == 0 { Some((i as u32 + 1) * 100) } else { None };
            assert_eq!(unsafe { r.add(&mut t, 0, i, row(i), order) }, OK);
        }
        assert_eq!(order(&t, r.len()), hashes(&["samples", "machines", "fx", "lfo", "shortcuts", "project"]));
    }

    check(&mut [0, 1, 2, 3, 4, 5], 0);
}

#[test]
fn a_name_that_has_an_owner_or_does_not_fit_is_refused() {
    let mut t = [empty(); 1 + PROVIDER_MAX];
    t[0] = fixed!("a");
    let mut r = Rows::new();
    unsafe {
        // an in-crate service is not provided twice, and a loaded one has one owner
        assert_eq!(r.add(&mut t, 1, 0, old!("a"), None), E_PLAN_OWNER);
        assert_eq!(r.add(&mut t, 1, 0, old!("b"), None), OK);
        assert_eq!(r.add(&mut t, 1, 1, old!("b"), None), E_PLAN_OWNER);
        assert_eq!(r.len(), 1);
        // the room is PROVIDER_MAX
        let names = [old!("c"), old!("d"), old!("e"), old!("f"), old!("g"), old!("h"), old!("i"), old!("j")];
        let mut owner = 2;
        for name in names {
            if r.len() == PROVIDER_MAX {
                break;
            }
            assert_eq!(r.add(&mut t, 1, owner, name, None), OK);
            owner += 1;
        }
        assert_eq!(r.add(&mut t, 1, owner, old!("k"), None), E_PLAN_FIT);
    }
    assert_eq!(r.len(), PROVIDER_MAX);
}

// ---- both kinds of row in one table ------------------------------------------------------------------------------------------

static SEEN: Mutex<Vec<(&'static str, &'static str)>> = Mutex::new(Vec::new());
fn seen(who: &'static str, what: &'static str) {
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).push((who, what));
}
fn take_seen() -> Vec<(&'static str, &'static str)> {
    std::mem::take(&mut *SEEN.lock().unwrap_or_else(|e| e.into_inner()))
}

/// An old row that logs the module begin and commit; `$refuses` makes its commit answer that status.
macro_rules! old_logging {
    ($ty:ident, $name:literal, $commit:expr) => {{
        struct $ty;
        impl<C: CoreApi> Service<C> for $ty {
            dkrt_abi::accepts_records!();
            dkrt_abi::accepts_teardown!();
            unsafe fn module_begin(_: &C, _: usize) {
                seen($name, "begin");
            }
            unsafe fn module_commit(_: &C, _: usize, _: &dkrt_abi::service::ModuleCommitCtx) -> i32 {
                seen($name, "commit");
                $commit
            }
        }
        static R: ServiceRow = ServiceRow::of::<$ty, RealCore>($name, &[]);
        RowSource::Service(&R)
    }};
}

/// A provider that adopts the lifecycle and the records part and logs the module begin and commit (and the tick, when `$tick`).
macro_rules! provider_logging {
    ($ty:ident, $name:literal, $commit:expr) => {{
        struct $ty;
        impl<C: CoreApi> Provider<C> for $ty {
            fn start(_: &C) -> Result<Self, i32> {
                Ok($ty)
            }
        }
        impl<C: CoreApi> Records<C> for $ty {
            fn begin(&mut self, _: &C, _: usize) {
                seen($name, "begin");
            }
            fn commit(&mut self, _: &C, _: usize, _: &dkrt_abi::service::ModuleCommitCtx) -> Result<(), i32> {
                seen($name, "commit");
                match $commit {
                    OK => Ok(()),
                    e => Err(e),
                }
            }
        }
        impl<C: CoreApi> Ticker<C> for $ty {
            fn tick(&mut self, _: &C, _: u32) {
                seen($name, "tick");
            }
        }
        static LIFE: LifecycleV1 = LifecycleV1::of::<$ty, RealCore>();
        static REC: RecordsV1 = RecordsV1::of::<$ty, RealCore>(&[]);
        static TICK: TickV1 = TickV1::of::<$ty, RealCore>();
        static PARTS: [PartRef; 3] = [
            PartRef::new(PART_LIFECYCLE, LIFECYCLE_NEWEST, &LIFE as *const _ as *const ()),
            PartRef::new(PART_RECORDS, RECORDS_NEWEST, &REC as *const _ as *const ()),
            PartRef::new(PART_TICK, TICK_NEWEST, &TICK as *const _ as *const ()),
        ];
        static ROW: ProviderRow = ProviderRow::new(name_hash($name), &PARTS);
        provider_source(&ROW)
    }};
}

/// The source of a provider row: its instance is a leaked box and the extent the whole address space (a host test's statics are in no package image).
fn provider_source(row: &'static ProviderRow) -> RowSource<'static> {
    RowSource::Provider { row, instance: std::boxed::Box::leak(std::boxed::Box::new([0u8; 8])).as_mut_ptr(), extent: 0..usize::MAX }
}

/// A provider that adopts the lifecycle part alone.
fn bare_provider(name: &'static str) -> RowSource<'static> {
    struct Bare;
    impl<C: CoreApi> Provider<C> for Bare {
        fn start(_: &C) -> Result<Self, i32> {
            Ok(Bare)
        }
    }
    static LIFE: LifecycleV1 = LifecycleV1::of::<Bare, RealCore>();
    static PARTS: [PartRef; 1] = [PartRef::new(PART_LIFECYCLE, LIFECYCLE_NEWEST, &LIFE as *const _ as *const ())];
    // one row per name: leaked
    provider_source(std::boxed::Box::leak(std::boxed::Box::new(ProviderRow::new(name_hash(name), &PARTS))))
}

fn services_over(rows: &'static [Row]) -> ServiceTable {
    ServiceTable::new(rows, &REAL_CORE)
}

/// A table filled through `Rows::add` (so the order is the table's own), leaked for the walk.
fn table_of(adds: Vec<(RowSource<'static>, Option<u32>)>) -> &'static [Row] {
    let t: &'static mut [Row] = std::boxed::Box::leak(std::vec![empty(); PROVIDER_MAX].into_boxed_slice());
    let mut r = Rows::new();
    for (i, (row, order)) in adds.into_iter().enumerate() {
        assert_eq!(unsafe { r.add(t, 0, i, row, order) }, OK);
    }
    &t[..r.len()]
}

fn commit_ctx<'a>(keys: &'a dkrt_abi::keys::KeysCopy, sink: &'a mut dkrt_abi::service::RecordSink) -> dkrt_abi::service::ModuleCommitCtx<'a> {
    dkrt_abi::service::ModuleCommitCtx { phase: dkrt_abi::service::commit_phase::PREPARE, base: 0x1000, image_size: 32, keys, sink }
}

#[test]
fn old_and_provider_rows_walk_in_callback_order_whatever_the_load_order() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    // the orders interleave the two kinds; the adds come in another order
    let t = table_of(std::vec![
        (provider_logging!(P300, "p300", OK), Some(300)),
        (old_logging!(O200, "o200", OK), Some(200)),
        (provider_logging!(P100, "p100", OK), Some(100)),
        (old_logging!(O400, "o400", OK), Some(400)),
    ]);
    take_seen();
    let keys = dkrt_abi::keys::KeysCopy::empty();
    let mut sink = dkrt_abi::service::RecordSink::measure();
    let ctx = commit_ctx(&keys, &mut sink);
    let table = services_over(t);
    unsafe {
        table.module_begin(3);
        assert_eq!(table.module_commit(3, &ctx), OK);
        table.tick(0);
    }
    let walked: Vec<&str> = take_seen().into_iter().filter(|s| s.1 == "begin").map(|s| s.0).collect();
    assert_eq!(walked, ["p100", "o200", "p300", "o400"]);
}

#[test]
fn the_first_refused_commit_stops_the_walk_whichever_kind_refuses() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    for provider_refuses in [false, true] {
        let refuser = if provider_refuses {
            provider_logging!(Pr, "refuses", dkrt_sdk::E_TYPE_USED)
        } else {
            old_logging!(Or, "refuses", dkrt_sdk::E_TYPE_USED)
        };
        let t = table_of(std::vec![
            (provider_logging!(Pf, "first", OK), Some(100)),
            (refuser, Some(200)),
            (old_logging!(Ol, "last", OK), Some(300)),
        ]);
        take_seen();
        let keys = dkrt_abi::keys::KeysCopy::empty();
        let mut sink = dkrt_abi::service::RecordSink::measure();
        let ctx = commit_ctx(&keys, &mut sink);
        unsafe {
            services_over(t).module_begin(3);
            assert_eq!(services_over(t).module_commit(3, &ctx), dkrt_sdk::E_TYPE_USED);
        }
        assert_eq!(
            take_seen(),
            [("first", "begin"), ("refuses", "begin"), ("last", "begin"), ("first", "commit"), ("refuses", "commit")],
            "provider refuses: {provider_refuses}: every row hears begin; the refusal stops the commit before the last row"
        );
    }
}

#[test]
fn a_part_the_provider_did_not_adopt_is_skipped() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let t = table_of(std::vec![(bare_provider("bare"), Some(100)), (old_logging!(Ob, "after", OK), Some(200))]);
    take_seen();
    let keys = dkrt_abi::keys::KeysCopy::empty();
    let mut sink = dkrt_abi::service::RecordSink::measure();
    let ctx = commit_ctx(&keys, &mut sink);
    let table = services_over(t);
    unsafe {
        table.module_begin(3);
        assert_eq!(table.module_commit(3, &ctx), OK);
        table.tick(0);
        assert_eq!(table.cap(1), None);
        assert!(table.dsp_clear(3));
    }
    assert_eq!(take_seen(), [("after", "begin"), ("after", "commit")]);
}

#[test]
fn a_part_version_newer_than_the_core_knows_is_refused_at_add_and_changes_nothing() {
    struct Newer;
    impl<C: CoreApi> Provider<C> for Newer {
        fn start(_: &C) -> Result<Self, i32> {
            Ok(Newer)
        }
    }
    static LIFE: LifecycleV1 = LifecycleV1::of::<Newer, RealCore>();
    static TICK: TickV1 = TickV1::of::<Newer, RealCore>();
    impl<C: CoreApi> Ticker<C> for Newer {
        fn tick(&mut self, _: &C, _: u32) {}
    }
    static PARTS: [PartRef; 2] = [
        PartRef::new(PART_LIFECYCLE, LIFECYCLE_NEWEST, &LIFE as *const _ as *const ()),
        PartRef::new(PART_TICK, TICK_NEWEST + 1, &TICK as *const _ as *const ()),
    ];
    static ROW: ProviderRow = ProviderRow::new(name_hash("newer"), &PARTS);
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 0, 0, provider_source(&ROW), None), E_SVC_ABI);
        assert_eq!(r.len(), 0);
        assert!(matches!(t[0], Row::Service(p) if p.is_null()));
        // the same row at a version the core knows is taken
        assert_eq!(r.add(&mut t, 0, 0, bare_provider("newer"), None), OK);
    }
    assert_eq!(r.len(), 1);
}

#[test]
fn a_provider_row_is_found_by_its_name_hash_and_has_one_owner() {
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 0, 0, bare_provider("same"), None), OK);
        // an old row of the same name is the same service
        assert_eq!(r.add(&mut t, 0, 1, old!("same"), None), E_PLAN_OWNER);
        assert_eq!(r.add(&mut t, 0, 1, bare_provider("same"), None), E_PLAN_OWNER);
    }
    assert_eq!(r.len(), 1);
    assert_eq!(r.index_of(0), Some(0));
}
