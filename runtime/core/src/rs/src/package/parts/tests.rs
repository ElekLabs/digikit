use super::*;
use core::sync::atomic::{AtomicU32, Ordering};

static STARTS: AtomicU32 = AtomicU32::new(0);
static STEPS: AtomicU32 = AtomicU32::new(0);
static STOPS: AtomicU32 = AtomicU32::new(0);

unsafe extern "C" fn start(env: *const PartEnv) -> i32 {
    assert_eq!((*env).size as usize, core::mem::size_of::<PartEnv>());
    STARTS.fetch_add(1, Ordering::SeqCst);
    0
}
unsafe extern "C" fn step(_: *const PartEnv) -> i32 {
    STEPS.fetch_add(1, Ordering::SeqCst);
    0
}
unsafe extern "C" fn stop(_: *const PartEnv) -> i32 {
    STOPS.fetch_add(1, Ordering::SeqCst);
    0
}
unsafe extern "C" fn start_fails(_: *const PartEnv) -> i32 {
    -1
}

static GOOD: PartTable = PartTable { magic: TABLE_MAGIC, name_hash: 7, start, step, stop };
static FAILING: PartTable = PartTable { magic: TABLE_MAGIC, name_hash: 9, start: start_fails, step, stop };
static NOT_A_TABLE: PartTable = PartTable { magic: 0, name_hash: 11, start, step, stop };

fn dir(tables: &[&'static PartTable]) -> PartDirectory {
    let mut d = PartDirectory { magic: PARTS_MAGIC, tables: [core::ptr::null(); MAX_PARTS] };
    for (i, t) in tables.iter().enumerate() {
        d.tables[i] = *t;
    }
    d
}

#[test]
fn the_package_without_a_part_calls_nothing() {
    let p = Parts::linked();
    assert_eq!(p.count(), 0);
    unsafe {
        assert_eq!(p.start(), 0);
        p.step();
    }
}

#[test]
fn every_part_is_found_by_its_name_hash_and_driven_through_its_table() {
    let d = dir(&[&GOOD, &FAILING]);
    let p = Parts::over(&d);
    assert_eq!(p.count(), 2);
    assert_eq!(p.find(7).map(|t| t.name_hash), Some(7));
    assert!(p.find(8).is_none());
    let (a, b, c) = (STARTS.load(Ordering::SeqCst), STEPS.load(Ordering::SeqCst), STOPS.load(Ordering::SeqCst));
    unsafe {
        assert_eq!(p.start(), 1, "one part answered an error");
        p.step();
        p.stop();
    }
    assert_eq!(STARTS.load(Ordering::SeqCst), a + 1);
    assert_eq!(STEPS.load(Ordering::SeqCst), b + 2);
    assert_eq!(STOPS.load(Ordering::SeqCst), c + 2);
}

#[test]
fn a_slot_without_the_table_magic_is_never_called() {
    let d = dir(&[&NOT_A_TABLE, &GOOD]);
    let p = Parts::over(&d);
    assert_eq!(p.count(), 1);
    assert!(p.find(11).is_none());
}
