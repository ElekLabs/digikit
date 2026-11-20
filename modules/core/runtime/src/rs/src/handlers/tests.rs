//! `FakeService`: a `Service` that records what the core calls (record kind, owner, bytes of `check`, `apply` and `remove`; the
//! declarations, attributions and `on_loaded`), and host tests of the core's walkers over it (docs/ARCHITECTURE.md "Boundaries and tests").
use super::*;
use dkrt_sdk::*;
use crate::registry::{apply_with, check_with, each, remove_with};
use dkrt_sdk::proto::CAPK_FX_SLOTS;
use crate::core_api::{CoreApi, REAL_CORE};
use dkrt_abi::service::ServiceRow;
use std::vec::Vec;

/// Serializes the tests that use the fake's log and the registry's state.
pub(crate) use crate::test_lock::LOCK;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Call {
    Check { svc: u8, step: u32, kind: u32, body: Vec<u8> },
    /// The rows a service writes before the per-record pass.
    Bulk { svc: u8, owner: u8 },
    Apply { svc: u8, owner: u8, kind: u32, body: Vec<u8> },
    Remove { svc: u8, owner: u8 },
    Attribute { svc: u8, site: u32, id: u32 },
    OnLoaded { svc: u8, rec: usize },
    Tick { svc: u8, phase: u32 },
    Diag { svc: u8, first: u32 },
    TableCheck { svc: u8, used: u32 },
}

static mut LOG: Vec<Call> = Vec::new();
/// `(svc, kind)`: the service that refuses a record of that kind in `check`.
static mut REFUSE: (u8, u32) = (0, 0);
/// The service whose `apply` fails.
static mut FAIL_APPLY: (u8, u32) = (0, 0);
/// The service whose `table_check` reports a mismatch (0: none).
static mut BAD_TABLE: u8 = 0;

pub(crate) fn log() -> &'static mut Vec<Call> {
    unsafe { &mut *core::ptr::addr_of_mut!(LOG) }
}

/// The fake service number `N` owns record kind `100 + N`, a record of `8 + 4 * N` bytes.
pub(crate) struct Fake<const N: u8>;

pub(crate) const fn kind_of(n: u8) -> u32 {
    100 + n as u32
}

unsafe fn body(reg: *const u8, kind: u32, b: *const u8) -> Vec<u8> {
    let _ = (reg, kind);
    core::slice::from_raw_parts(b, 4 * (kind as usize - 100)).to_vec()
}

impl<const N: u8, C: CoreApi> Service<C> for Fake<N> {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();

    /// Service 1 checks its records at the step of native pages, service 2 at the step of effects: the order of the old body.
    unsafe fn check(_: &C, step: u32, c: &CheckCtx) -> i32 {
        let mine = if N == 1 { check_step::NATIVE } else { check_step::EFFECTS };
        let mut r = OK;
        if step != mine {
            return r;
        }
        each(c.reg, |kind, b| {
            if kind == kind_of(N) && r == OK {
                log().push(Call::Check { svc: N, step, kind, body: body(c.reg, kind, b) });
                if REFUSE == (N, kind) {
                    r = E_FORMAT;
                }
            }
        });
        r
    }

    unsafe fn apply(_: &C, c: &ApplyCtx) -> i32 {
        log().push(Call::Bulk { svc: N, owner: c.owner });
        OK
    }

    unsafe fn apply_record(_: &C, kind: u32, b: *const u8, c: &ApplyCtx) -> i32 {
        if kind != kind_of(N) {
            return OK;
        }
        log().push(Call::Apply { svc: N, owner: c.owner, kind, body: body(c.reg, kind, b) });
        if FAIL_APPLY == (N, kind) {
            E_FORMAT
        } else {
            OK
        }
    }

    unsafe fn remove(_: &C, owner: u8) {
        log().push(Call::Remove { svc: N, owner });
    }

    unsafe fn cap(_: &C, kind: u8) -> Option<(u32, u32)> {
        (kind == CAPK_FX_SLOTS).then_some((10 * N as u32, N as u32))
    }

    unsafe fn attribute(_: &C, site: u32, id: u32) -> Option<usize> {
        log().push(Call::Attribute { svc: N, site, id });
        if site == N as u32 {
            Some(id as usize + 100 * N as usize)
        } else {
            None
        }
    }

    unsafe fn on_loaded(_: &C, rec: usize) {
        log().push(Call::OnLoaded { svc: N, rec });
    }

    unsafe fn tick(_: &C, phase: u32) {
        log().push(Call::Tick { svc: N, phase });
    }

    unsafe fn diag(_: &C, words: &[u32; dkrt_sdk::fault::DIAG_WORDS]) {
        log().push(Call::Diag { svc: N, first: words[0] });
    }

    unsafe fn table_check(_: &C, used: u32, out: &mut dkrt_abi::service::TableFault) -> u32 {
        log().push(Call::TableCheck { svc: N, used });
        if BAD_TABLE == N {
            *out = dkrt_abi::service::TableFault { module: N as u32, a: 7, b: used };
            return 1;
        }
        0
    }
}

/// Fake service `N` owns record kind `100 + N`, a record of `8 + 4 * N` bytes.
static KINDS_1: [(u32, u32); 1] = [(101, 12)];
static KINDS_2: [(u32, u32); 1] = [(102, 16)];
static TWO_ROWS: [ServiceRow; 2] = [ServiceRow::of::<Fake<1>, crate::core_api::RealCore>("fake1", &KINDS_1), ServiceRow::of::<Fake<2>, crate::core_api::RealCore>("fake2", &KINDS_2)];
static TWO_ROWS_P: dkrt_abi::service::RowPtrs<2> = dkrt_abi::service::RowPtrs::of(&TWO_ROWS);
pub(crate) const TWO: ServiceTable = ServiceTable::new(&TWO_ROWS_P.0, &REAL_CORE);

/// A `register` section of the given records `(kind, 4 * (kind - 100) body bytes filled with the record number)`.
fn section(kinds: &[u32]) -> Vec<u8> {
    let mut v = Vec::new();
    v.extend_from_slice(&0x444b_5247u32.to_be_bytes());
    v.extend_from_slice(&1u32.to_be_bytes());
    v.extend_from_slice(&1u32.to_be_bytes()); // the device
    v.extend_from_slice(&(kinds.len() as u32).to_be_bytes());
    for (i, &k) in kinds.iter().enumerate() {
        let n = 4 * (k as usize - 100);
        v.extend_from_slice(&k.to_be_bytes());
        v.extend_from_slice(&(8 + n as u32).to_be_bytes());
        v.extend(core::iter::repeat(i as u8 + 1).take(n));
    }
    v
}

unsafe fn reset() {
    log().clear();
    REFUSE = (0, 0);
    FAIL_APPLY = (0, 0);
    BAD_TABLE = 0;
}

fn rec(svc: u8, i: u8) -> (u32, Vec<u8>) {
    (kind_of(svc), vec![i; 4 * svc as usize])
}

#[test]
fn a_mixed_file_checks_in_step_order_applies_in_file_order_and_removes_in_reverse() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        // mixed kinds: service 1, 2, 1
        let sec = section(&[kind_of(1), kind_of(2), kind_of(1)]);
        assert_eq!(check_with(TWO, sec.as_ptr(), sec.len() as u32, 0, 1), OK);
        assert_eq!(apply_with(TWO, 5, sec.as_ptr(), sec.len() as u32, 0x1000), OK);
        remove_with(TWO, 5);
        let (k1, b1) = rec(1, 1);
        let (k1b, b1b) = rec(1, 3);
        let (k2, b2) = rec(2, 2);
        let n = check_step::NATIVE;
        let e = check_step::EFFECTS;
        assert_eq!(
            *log(),
            vec![
                // check: step by step, so service 1's step (native pages) before service 2's (effects), its records in file order
                Call::Check { svc: 1, step: n, kind: k1, body: b1.clone() },
                Call::Check { svc: 1, step: n, kind: k1b, body: b1b.clone() },
                Call::Check { svc: 2, step: e, kind: k2, body: b2.clone() },
                // apply: every service's rows first, then each record in file order across the kinds
                Call::Bulk { svc: 1, owner: 5 },
                Call::Bulk { svc: 2, owner: 5 },
                Call::Apply { svc: 1, owner: 5, kind: k1, body: b1 },
                Call::Apply { svc: 2, owner: 5, kind: k2, body: b2 },
                Call::Apply { svc: 1, owner: 5, kind: k1b, body: b1b },
                // remove: the last service first
                Call::Remove { svc: 2, owner: 5 },
                Call::Remove { svc: 1, owner: 5 },
            ]
        );
    }
}

#[test]
fn the_first_fault_is_the_first_step_that_finds_one_and_a_failed_apply_stops_the_file() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        // an effect-step fault precedes nothing: the native step runs first, as in the old body, wherever the records sit in the file
        let sec = section(&[kind_of(2), kind_of(1), kind_of(2)]);
        REFUSE = (2, kind_of(2));
        assert_eq!(check_with(TWO, sec.as_ptr(), sec.len() as u32, 0, 1), E_FORMAT);
        assert_eq!(log().len(), 2); // service 1's record, then service 2's first (refused) record; the second is not looked at
        assert!(matches!(log()[0], Call::Check { svc: 1, .. }));
        reset();
        REFUSE = (1, kind_of(1));
        assert_eq!(check_with(TWO, sec.as_ptr(), sec.len() as u32, 0, 1), E_FORMAT);
        assert_eq!(log().len(), 1, "service 1 refused at its step, so service 2's step never ran");

        // apply stops at the first failing record in file order; later records of any kind are not applied
        reset();
        FAIL_APPLY = (1, kind_of(1));
        assert_eq!(apply_with(TWO, 5, sec.as_ptr(), sec.len() as u32, 0), E_FORMAT);
        let applied: Vec<_> = log().iter().filter(|c| matches!(c, Call::Apply { .. })).collect();
        assert_eq!(applied.len(), 2, "the effect record before the failing one was applied, the one after was not");
    }
}

#[test]
fn the_walker_refuses_a_kind_no_service_declared_and_a_wrong_length() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        // kind 99 is no one's
        let mut bad = section(&[kind_of(1)]);
        bad[12..16].copy_from_slice(&2u32.to_be_bytes());
        bad.extend_from_slice(&99u32.to_be_bytes());
        bad.extend_from_slice(&8u32.to_be_bytes());
        assert_eq!(check_with(TWO, bad.as_ptr(), bad.len() as u32, 0, 1), E_FORMAT);
        assert!(log().is_empty());
        let mut wrong = section(&[kind_of(2)]);
        wrong[20..24].copy_from_slice(&12u32.to_be_bytes()); // the record's length word: a kind-1 length
        assert_eq!(check_with(TWO, wrong.as_ptr(), wrong.len() as u32, 0, 1), E_FORMAT);
        assert!(log().is_empty());
    }
}

#[test]
fn capacity_sums_the_declared_counts() {
    unsafe {
        // 10 + 20 slots, 1 + 2 used: two services declare the same kind
        assert_eq!(TWO.cap(CAPK_FX_SLOTS), Some((30, 3)));
        assert_eq!(TWO.cap(CAPK_FX_SLOTS + 1), None);
    }
}

#[test]
fn attribution_asks_the_services_in_order_until_one_answers() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        assert_eq!(TWO.attribute(2, 7), Some(207));
        assert_eq!(
            *log(),
            vec![Call::Attribute { svc: 1, site: 2, id: 7 }, Call::Attribute { svc: 2, site: 2, id: 7 }]
        );
        assert_eq!(TWO.attribute(9, 7), None);
    }
}

#[test]
fn the_diagnostics_block_reaches_every_service_in_list_order() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        let mut words = [0u32; dkrt_sdk::fault::DIAG_WORDS];
        words[0] = 0xd1a6;
        TWO.diag(&words);
        assert_eq!(*log(), vec![Call::Diag { svc: 1, first: 0xd1a6 }, Call::Diag { svc: 2, first: 0xd1a6 }]);
    }
}

#[test]
fn the_table_check_stops_at_the_first_service_that_reports_and_gives_its_numbers() {
    use dkrt_abi::service::TableFault;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        let mut out = TableFault::default();
        assert_eq!(TWO.table_check(0b101, &mut out), 0, "tables that agree");
        assert_eq!(out, TableFault::default());
        assert_eq!(*log(), vec![Call::TableCheck { svc: 1, used: 5 }, Call::TableCheck { svc: 2, used: 5 }]);
        log().clear();
        BAD_TABLE = 1;
        assert_eq!(TWO.table_check(0b101, &mut out), 1);
        assert_eq!(out, TableFault { module: 1, a: 7, b: 5 });
        assert_eq!(*log(), vec![Call::TableCheck { svc: 1, used: 5 }], "the walk ends at the first report");
    }
}

#[test]
fn a_tick_phase_reaches_every_service_in_list_order() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        reset();
        TWO.tick(tick_phase::EARLY);
        TWO.tick(tick_phase::LATE);
        assert_eq!(
            *log(),
            vec![
                Call::Tick { svc: 1, phase: tick_phase::EARLY },
                Call::Tick { svc: 2, phase: tick_phase::EARLY },
                Call::Tick { svc: 1, phase: tick_phase::LATE },
                Call::Tick { svc: 2, phase: tick_phase::LATE },
            ]
        );
    }
}

/// A service that declares a kind its table does not carry, one that declares a wrong length, and one that matches.
struct Good;
impl<C: CoreApi> Service<C> for Good {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(70, 16);
    }
}
struct WrongLen;
impl<C: CoreApi> Service<C> for WrongLen {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(71, 20);
    }
}
struct Extra;
impl<C: CoreApi> Service<C> for Extra {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(72, 8);
        d.slot(1, 0, 0);
    }
}
struct Silent;
impl<C: CoreApi> Service<C> for Silent {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
}

static DECL_ROWS: [ServiceRow; 4] = [
    ServiceRow::of::<Good, crate::core_api::RealCore>("good", &[(70, 16)]),
    ServiceRow::of::<WrongLen, crate::core_api::RealCore>("wronglen", &[(71, 16)]),
    ServiceRow::of::<Extra, crate::core_api::RealCore>("extra", &[(72, 8)]),
    ServiceRow::of::<Silent, crate::core_api::RealCore>("silent", &[(73, 8)]),
];
static DECL_ROWS_P: dkrt_abi::service::RowPtrs<4> = dkrt_abi::service::RowPtrs::of(&DECL_ROWS);

#[test]
fn a_service_whose_declarations_differ_from_its_table_is_failed_and_its_kinds_are_unknown() {
    let t = ServiceTable::new(&DECL_ROWS_P.0, &REAL_CORE);
    let failed = unsafe { t.check_declarations() };
    assert_eq!(failed, 0b1110, "the matching service passes; a wrong length, an extra item and a missing declaration fail");
    let t = ServiceTable { failed, ..t };
    assert_eq!(t.len(70), Some(16));
    for kind in [71, 72, 73] {
        assert_eq!(t.len(kind), None);
        assert!(t.owner(kind).is_none());
    }
}

#[test]
fn the_in_crate_services_declare_what_the_generated_table_carries() {
    // the real rows: samples owns none, machines six kinds, fx one
    assert_eq!(unsafe { crate::gen_services::TABLE.check_declarations() }, 0);
}

mod table {
    use super::*;

    struct A;
    impl<C: CoreApi> Service<C> for A {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();
    }
    struct B;
    impl<C: CoreApi> Service<C> for B {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();

        unsafe fn dsp_clear(_: &C, _: usize) -> bool {
            false
        }
        unsafe fn cap(_: &C, kind: u8) -> Option<(u32, u32)> {
            (kind == 3).then_some((6, 1))
        }
    }

    static ROWS: [ServiceRow; 2] = [ServiceRow::of::<A, crate::core_api::RealCore>("a", &[(1, 8)]), ServiceRow::of::<B, crate::core_api::RealCore>("b", &[(2, 12)])];
    static ROWS_P: dkrt_abi::service::RowPtrs<2> = dkrt_abi::service::RowPtrs::of(&ROWS);
    const T: ServiceTable = ServiceTable::new(&ROWS_P.0, &REAL_CORE);

    #[test]
    fn a_table_asks_each_service_and_ands_the_clears() {
        assert_eq!(T.len(1), Some(8));
        assert_eq!(T.len(2), Some(12));
        assert_eq!(T.len(3), None);
        assert_eq!(T.owner(2).map(|r| r.name()), Some("b"));
        assert!(T.owner(9).is_none());
        unsafe {
            assert!(!T.dsp_clear(0));
            assert!(ServiceTable { rows: &ROWS_P.0[..1], ..T }.dsp_clear(0));
        }
    }

    use core::sync::atomic::{AtomicU32, Ordering::SeqCst};

    /// What each service of the abort case did: the number of `dsp_clear` calls it saw, and (the first) how many calls still fail.
    static FAILS_LEFT: AtomicU32 = AtomicU32::new(0);
    static CLEARS: [AtomicU32; 3] = [AtomicU32::new(0), AtomicU32::new(0), AtomicU32::new(0)];

    macro_rules! counting {
        ($name:ident, $slot:expr, $fails:expr) => {
            struct $name;
            impl<C: CoreApi> Service<C> for $name {
                dkrt_abi::accepts_records!();
                dkrt_abi::accepts_teardown!();
                unsafe fn dsp_clear(_: &C, _: usize) -> bool {
                    CLEARS[$slot].fetch_add(1, SeqCst);
                    if $fails && FAILS_LEFT.load(SeqCst) > 0 {
                        FAILS_LEFT.fetch_sub(1, SeqCst);
                        return false;
                    }
                    true
                }
            }
        };
    }
    counting!(First, 0, false);
    counting!(Failing, 1, true);
    counting!(Last, 2, false);

    /// A module with rows in several services (a machine with sample pool entries): the DSP clear of an unload goes to every service even when one of them fails
    /// (E5c), so a service that comes after the failing one has cleared its rows too. The unload then answers `E_BUSY` before `fini` and the job gives the module
    /// back (`reload_abort`); the next try clears every service again and succeeds.
    #[test]
    fn the_dsp_clear_reaches_every_service_when_one_fails_and_a_second_try_after_the_abort_clears_all_again() {
        static MULTI: [ServiceRow; 3] = [
            ServiceRow::of::<First, crate::core_api::RealCore>("first", &[]),
            ServiceRow::of::<Failing, crate::core_api::RealCore>("failing", &[]),
            ServiceRow::of::<Last, crate::core_api::RealCore>("last", &[]),
        ];
        static MULTI_P: dkrt_abi::service::RowPtrs<3> = dkrt_abi::service::RowPtrs::of(&MULTI);
        let t = ServiceTable { rows: &MULTI_P.0, ..T };
        FAILS_LEFT.store(1, SeqCst);
        for c in &CLEARS {
            c.store(0, SeqCst);
        }
        unsafe {
            assert!(!t.dsp_clear(0), "the failing service holds the unload");
            assert_eq!(CLEARS.iter().map(|c| c.load(SeqCst)).collect::<Vec<_>>(), [1, 1, 1], "the service after the failing one cleared too");
            // after the abort the job tries again from the same record
            assert!(t.dsp_clear(0));
            assert_eq!(CLEARS.iter().map(|c| c.load(SeqCst)).collect::<Vec<_>>(), [2, 2, 2], "every service clears again; a clear of rows already clear is harmless");
        }
    }

    #[test]
    fn a_kind_two_services_count_is_the_sum_of_their_counts() {
        static TWO_B: [ServiceRow; 2] = [ServiceRow::of::<B, crate::core_api::RealCore>("b1", &[]), ServiceRow::of::<B, crate::core_api::RealCore>("b2", &[])];
        static TWO_B_P: dkrt_abi::service::RowPtrs<2> = dkrt_abi::service::RowPtrs::of(&TWO_B);
        unsafe {
            assert_eq!(ServiceTable { rows: &TWO_B_P.0, ..T }.cap(3), Some((12, 2)));
            assert_eq!(T.cap(3), Some((6, 1)));
            assert_eq!(T.cap(4), None);
        }
    }
}

/// What only the runtime asks of a service file goes through the loader's export table by name: the effect list of DIGIKIT > FX ROUTES, the guard counts and the
/// machine types of the MODULES reply. With no file loaded (nothing published) the answers are "no fx", "no counts" and "no types"; with a file loaded the export answers.
#[test]
fn the_runtime_only_exports_answer_from_the_service_file_and_say_nothing_without_one() {
    use super::exports::*;
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    extern "C" fn fx(index: u32, out: *mut u8) -> i32 {
        if index < 2 {
            unsafe { (*(out as *mut DkrtFx)).row = 7 + 2 * index };
            OK
        } else {
            E_NOT_FOUND
        }
    }
    extern "C" fn info(owner: u32, out: *mut u8) -> u32 {
        let m = dkrt_abi::service::ModuleMachines { rows: 1 << owner, nt: 1, types: [16 + owner as u8, 0, 0, 0, 0, 0, 0, 0], faults: owner + 1, guard_off: (owner == 3) as u32 };
        unsafe { *(out as *mut dkrt_abi::service::ModuleMachines) = m };
        (owner < 8) as u32
    }
    unsafe {
        let mut out: DkrtFx = core::mem::zeroed();
        TEST_EXPORTS.lock().unwrap_or_else(|e| e.into_inner()).clear();
        assert_eq!(fx_entry(0, &mut out), E_NOT_FOUND, "no fx without the service file");
        assert!(module_info(1).is_none(), "no machines without the service file");
        {
            let mut t = TEST_EXPORTS.lock().unwrap_or_else(|e| e.into_inner());
            t.push((dkneeds::keys::name_hash(FX_ROUTES_ENTRY), fx as usize));
            t.push((dkneeds::keys::name_hash(MACHINES_MODULE_INFO), info as usize));
        }
        assert_eq!(fx_entry(0, &mut out), OK);
        assert_eq!(out.row, 7);
        assert_eq!(fx_entry(1, &mut out), OK);
        assert_eq!(out.row, 9);
        assert_eq!(fx_entry(2, &mut out), E_NOT_FOUND, "past the end");
        let m = module_info(3).unwrap();
        assert_eq!((m.rows, m.nt, m.types[0], m.faults, m.guard_off), (8, 1, 19, 4, 1));
        assert!(module_info(9).is_none(), "an owner the service does not answer for");
        TEST_EXPORTS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}
