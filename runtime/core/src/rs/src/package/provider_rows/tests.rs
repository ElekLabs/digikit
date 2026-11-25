use super::*;
use crate::package::core_api::{RealCore, REAL_CORE};
use crate::package::fixture_provider::Nothing;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::{
    name_hash, LifecycleV1, PartRef, Provider, Records, RecordsV1, TickV1, Ticker, LIFECYCLE_NEWEST, PART_LIFECYCLE, PART_RECORDS, PART_TICK, RECORDS_NEWEST, TICK_NEWEST,
};
use dkrt_sdk::E_SVC_ABI;
use std::sync::Mutex;
use std::vec::Vec;

/// A row named `$name`.
macro_rules! row {
    ($name:literal) => {
        Nothing::named($name).source()
    };
}

/// The name hashes of the first `n` rows of `t`, in table order.
fn order(t: &[ProviderHandle], n: usize) -> Vec<u32> {
    t[..n].iter().map(|r| r.name()).collect()
}

fn hashes(names: &[&str]) -> Vec<u32> {
    names.iter().map(|n| name_hash(n)).collect()
}

fn empty() -> ProviderHandle {
    ProviderHandle::EMPTY
}

#[test]
fn rows_with_the_same_order_use_the_name_hash_and_removal_closes_the_gap() {
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 3, row!("d"), None), OK);
        assert_eq!(r.add(&mut t, 1, row!("b"), None), OK);
        assert_eq!(r.add(&mut t, 2, row!("e"), None), OK);
    }
    // rows of the same order follow by the hash of their names
    let mut by_hash = ["b", "d", "e"];
    by_hash.sort_by_key(|n| name_hash(n));
    assert_eq!(order(&t, r.len()), hashes(&by_hash));
    // taking one out closes the gap and clears the end; a second remove does nothing
    r.remove(&mut t, 3);
    assert_eq!(r.len(), 2);
    assert!(!order(&t, r.len()).contains(&name_hash("d")));
    assert_eq!(t[r.len()].name(), 0, "the cell after the last row is empty");
    r.remove(&mut t, 3);
    r.remove(&mut t, 0);
    assert_eq!(r.len(), 2);
}

#[test]
fn the_explicit_maximum_precedes_an_unordered_service() {
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 0, row!("a"), None), OK);
        assert_eq!(r.add(&mut t, 1, row!("z"), Some(u32::MAX)), OK);
    }
    assert_eq!(order(&t, r.len()), hashes(&["z", "a"]));
}

#[test]
fn the_six_shipped_rows_keep_their_order_for_every_load_order() {
    // the callback order of each manifest (`callback_order` of the service's module.toml): samples, machines, fx, lfo, shortcuts, project
    const SHIPPED: [(&str, u32); 6] = [("samples", 100), ("machines", 200), ("fx", 300), ("lfo", 400), ("shortcuts", 500), ("project", 600)];

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
            let (name, order) = SHIPPED[i];
            let source = match name {
                "samples" => row!("samples"),
                "machines" => row!("machines"),
                "fx" => row!("fx"),
                "lfo" => row!("lfo"),
                "shortcuts" => row!("shortcuts"),
                _ => row!("project"),
            };
            assert_eq!(unsafe { r.add(&mut t, i, source, Some(order)) }, OK);
        }
        assert_eq!(order(&t, r.len()), hashes(&["samples", "machines", "fx", "lfo", "shortcuts", "project"]));
    }

    check(&mut [0, 1, 2, 3, 4, 5], 0);
}

#[test]
fn a_file_without_a_packed_order_follows_every_file_that_has_one_in_whatever_name_order() {
    // no name decides an order any more: a row with none sorts after the ordered ones, by name hash among themselves
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        for (rec, name) in ["samples", "machines", "fx"].into_iter().enumerate() {
            let source = match name {
                "samples" => row!("samples"),
                "machines" => row!("machines"),
                _ => row!("fx"),
            };
            assert_eq!(r.add(&mut t, rec, source, None), OK);
        }
        assert_eq!(r.add(&mut t, 3, row!("lfo"), Some(400)), OK);
    }
    assert_eq!(order(&t, 1)[0], name_hash("lfo"));
    let mut rest = ["samples", "machines", "fx"];
    rest.sort_by_key(|n| name_hash(n));
    assert_eq!(order(&t, r.len())[1..], hashes(&rest)[..]);
}

#[test]
fn a_name_that_has_an_owner_or_does_not_fit_is_refused() {
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        // a loaded service has one owner
        assert_eq!(r.add(&mut t, 0, row!("b"), None), OK);
        assert_eq!(r.add(&mut t, 1, row!("b"), None), E_PLAN_OWNER);
        assert_eq!(r.len(), 1);
        // the room is PROVIDER_MAX
        let names = [row!("c"), row!("d"), row!("e"), row!("f"), row!("g"), row!("h"), row!("i"), row!("j")];
        let mut owner = 2;
        for name in names {
            if r.len() == PROVIDER_MAX {
                break;
            }
            assert_eq!(r.add(&mut t, owner, name, None), OK);
            owner += 1;
        }
        assert_eq!(r.add(&mut t, owner, row!("k"), None), E_PLAN_FIT);
    }
    assert_eq!(r.len(), PROVIDER_MAX);
}

// ---- the walks over a table of rows ------------------------------------------------------------------------------------------

static SEEN: Mutex<Vec<(&'static str, &'static str)>> = Mutex::new(Vec::new());
fn seen(who: &'static str, what: &'static str) {
    SEEN.lock().unwrap_or_else(|e| e.into_inner()).push((who, what));
}
fn take_seen() -> Vec<(&'static str, &'static str)> {
    std::mem::take(&mut *SEEN.lock().unwrap_or_else(|e| e.into_inner()))
}

/// A provider that adopts the lifecycle and the records part and logs the module begin and commit (and the tick, when `$tick`).
macro_rules! provider_logging {
    ($ty:ident, $name:literal, $commit:expr) => {{
        struct $ty;
        impl<C: CoreApi> Provider<C> for $ty {
            fn start_in(_: &C, slot: &mut core::mem::MaybeUninit<Self>) -> Result<(), i32> {
                slot.write($ty);
                Ok(())
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
    RowSource { row, instance: std::boxed::Box::leak(std::boxed::Box::new([0u8; 8])).as_mut_ptr(), extent: 0..usize::MAX }
}

/// A provider that adopts the lifecycle part alone.
fn bare_provider(name: &'static str) -> RowSource<'static> {
    Nothing::named(name).source()
}

fn services_over(rows: &'static [ProviderHandle]) -> ServiceTable {
    ServiceTable::new(rows, &REAL_CORE)
}

/// A table filled through `Rows::add` (so the order is the table's own), leaked for the walk.
fn table_of(adds: Vec<(RowSource<'static>, Option<u32>)>) -> &'static [ProviderHandle] {
    let t: &'static mut [ProviderHandle] = std::boxed::Box::leak(std::vec![empty(); PROVIDER_MAX].into_boxed_slice());
    let mut r = Rows::new();
    for (i, (row, order)) in adds.into_iter().enumerate() {
        assert_eq!(unsafe { r.add(t, i, row, order) }, OK);
    }
    &t[..r.len()]
}

fn commit_ctx<'a>(keys: &'a dkrt_abi::keys::KeysCopy, sink: &'a mut dkrt_abi::service::RecordSink) -> dkrt_abi::service::ModuleCommitCtx<'a> {
    dkrt_abi::service::ModuleCommitCtx { phase: dkrt_abi::service::commit_phase::PREPARE, base: 0x1000, image_size: 32, keys, sink }
}

#[test]
fn rows_walk_in_callback_order_whatever_the_load_order() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    // the adds come in another order than the callback orders
    let t = table_of(std::vec![
        (provider_logging!(P300, "p300", OK), Some(300)),
        (provider_logging!(P200, "p200", OK), Some(200)),
        (provider_logging!(P100, "p100", OK), Some(100)),
        (provider_logging!(P400, "p400", OK), Some(400)),
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
    assert_eq!(walked, ["p100", "p200", "p300", "p400"]);
}

#[test]
fn the_first_refused_commit_stops_the_walk() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let t = table_of(std::vec![
        (provider_logging!(Pf, "first", OK), Some(100)),
        (provider_logging!(Pr, "refuses", dkrt_sdk::E_TYPE_USED), Some(200)),
        (provider_logging!(Pl, "last", OK), Some(300)),
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
        "every row hears begin; the refusal stops the commit before the last row"
    );
}

#[test]
fn a_part_the_provider_did_not_adopt_is_skipped() {
    let _g = crate::test_lock::LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let t = table_of(std::vec![(bare_provider("bare"), Some(100)), (provider_logging!(Pb, "after", OK), Some(200))]);
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
    assert_eq!(take_seen(), [("after", "begin"), ("after", "commit"), ("after", "tick")]);
}

#[test]
fn a_part_version_newer_than_the_core_knows_is_refused_at_add_and_changes_nothing() {
    struct Newer;
    impl<C: CoreApi> Provider<C> for Newer {
        fn start_in(_: &C, slot: &mut core::mem::MaybeUninit<Self>) -> Result<(), i32> {
            slot.write(Newer);
            Ok(())
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
        assert_eq!(r.add(&mut t, 0, provider_source(&ROW), None), E_SVC_ABI);
        assert_eq!(r.len(), 0);
        assert_eq!(t[0].name(), 0, "nothing was added");
        // the same row at a version the core knows is taken
        assert_eq!(r.add(&mut t, 0, bare_provider("newer"), None), OK);
    }
    assert_eq!(r.len(), 1);
}

#[test]
fn a_provider_row_is_found_by_its_name_hash_and_has_one_owner() {
    let mut t = [empty(); PROVIDER_MAX];
    let mut r = Rows::new();
    unsafe {
        assert_eq!(r.add(&mut t, 0, bare_provider("same"), None), OK);
        assert_eq!(r.add(&mut t, 1, bare_provider("same"), None), E_PLAN_OWNER);
    }
    assert_eq!(r.len(), 1);
    assert_eq!(r.index_of(0), Some(0));
}
