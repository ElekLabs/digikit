//! The service over a `FakeCore` and a `FakeFw`: the record checks, `apply` and `remove` of parameter rows and click actions, the native page
//! record and the capacity counts. The machine and page rows are written through `FwMem` (`wr`), so a host test reaches them: `tests/contract.rs` loads and
//! unloads whole modules and compares the rows (the contract suite); the tests here cover the single handlers.
use dkneeds::keys::name_hash;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::FwMem;
use dkrt_abi::service::*;
use dkrt_sdk::records::*;
use dkrt_sdk::*;
use dkrt_svc_machines::info;
use dkrt_svc_machines::records::*;
use dkrt_svc_machines::slots;
use dkrt_svc_params::dynreg::w;
use dkrt_test_support::{Call, FakeCore, FakeFw};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;
use std::vec::Vec;

/// The tests that drive the statics of the crate (the click state and `APPLIED`) take this lock.
static TEST_LOCK: Mutex<()> = Mutex::new(());

const P_FIRST: u32 = 100;
const P_DYN0: u32 = 200;
const SLOT_BASE: u32 = 10;
const OWNER: u8 = 3;
const BASE: u32 = 0x4000;

/// The words of `dynreg_info` that the parameter, action and native page parts read; no page rows (`PG_ROWS` 0).
fn table() -> Vec<u32> {
    let mut t = std::vec![0u32; 96];
    t[w::M_DYN_N] = 8;
    t[w::P_FIRST] = P_FIRST;
    t[w::P_DYN0] = P_DYN0;
    t[w::P_DYN_N] = 16;
    t[w::PG_DYN_N] = 4;
    t[w::KNOBS] = 8;
    t[w::SLOT_BASE] = SLOT_BASE;
    // the parameter table of the firmware (a fake: `FakeCore` keeps the words written there): rows of 0x40 bytes, the first three words are min, max and default
    t[w::P_TABLE] = P_TABLE;
    t[w::PF_SIZE] = 0x40;
    t[w::SPEC] = 0x100;
    t[w::SPEC + 1] = 0x101;
    t[w::SPEC + 2] = 0x102;
    t[w::P_ROWS] = 0x20000;
    t[w::P_ROW_SIZE] = 0x20;
    t[w::P_TEMPLATE_OFF] = 8;
    t
}

const P_TABLE: u32 = 0x10000;

/// The (min, max, default) words of the parameter table row `code`, as the writer left them.
fn row_words(core: &FakeCore, code: u32) -> [u32; 3] {
    let a = P_TABLE + code * 0x40;
    unsafe { [core.rd(a), core.rd(a + 4), core.rd(a + 8)] }
}

const N_REGISTER: u32 = 0x6000_0010;
const N_REMOVE: u32 = 0x6000_0030;
const N_TURN: u32 = 0x6000_0040;

/// The image table `machines_info` with the adapter's native page calls bound (`native_pages: false`: the words are 0).
fn machines_info(native_pages: bool) -> Vec<u32> {
    let mut t = std::vec![0u32; info::WORDS];
    t[info::w::MAGIC] = info::MAGIC;
    t[info::w::VERSION] = info::VERSION;
    if native_pages {
        t[info::w::N_REGISTER] = N_REGISTER;
        t[info::w::N_REGISTER_V2] = N_REGISTER + 0x10;
        t[info::w::N_REMOVE] = N_REMOVE;
        t[info::w::N_TURN] = N_TURN;
    }
    t
}

fn core() -> FakeCore {
    let mut c = FakeCore::new();
    c.set_dynreg(table());
    c.info_tables
        .push((name_hash("machines_info"), machines_info(true)));
    c.device = dkrt_abi::profiles::DT2.id;
    c
}

fn put(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

/// A parameter record: its index in the file, template, slot, range and default, with the long and the short name.
fn param(index: u32, slot: u32, min: i32, max: i32, def: u32, name: &str) -> (u32, Vec<u8>) {
    let mut b = std::vec![0u8; LEN_PARAM as usize - 8];
    put(&mut b, 0, index);
    put(&mut b, 4, 7);
    put(&mut b, 8, slot);
    put(&mut b, 12, min as u32);
    put(&mut b, 16, max as u32);
    put(&mut b, 20, def);
    b[24..24 + name.len()].copy_from_slice(name.as_bytes());
    b[56..56 + 2].copy_from_slice(b"PR");
    (KIND_PARAM, b)
}

/// An action record: the parameter (index in the file) whose click runs the callback at image offset `off`.
fn action(pi: u32, off: u32) -> (u32, Vec<u8>) {
    let mut b = std::vec![0u8; LEN_ACTION as usize - 8];
    put(&mut b, 0, pi);
    put(&mut b, 4, off);
    (KIND_ACTION, b)
}

/// A native page record (version 1) of machine key `key` with callbacks at image offsets 2 and 4 and one value binding.
fn native(key: u32) -> (u32, Vec<u8>) {
    let mut b = std::vec![0u8; LEN_NATIVE_PAGE as usize - 8];
    put(&mut b, 0, 1);
    put(&mut b, 4, key);
    put(&mut b, 8, 2);
    put(&mut b, 12, 4);
    b[16..20].copy_from_slice(b"PAGE");
    put(&mut b, 40, 2); // binding 0: a literal value (kind 2) at position 0
    put(&mut b, 44, 9);
    (KIND_NATIVE_PAGE, b)
}

/// A `register` section: the header words, then each record as kind, whole length and body.
fn section(records: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut s = std::vec![0u8; 16];
    put(&mut s, 12, records.len() as u32);
    for (kind, body) in records {
        let mut r = std::vec![0u8; 8];
        put(&mut r, 0, *kind);
        put(&mut r, 4, body.len() as u32 + 8);
        s.extend_from_slice(&r);
        s.extend_from_slice(body);
    }
    s
}

fn check_ctx(reg: &[u8]) -> CheckCtx {
    CheckCtx {
        reg: reg.as_ptr(),
        image_size: 0x100,
    }
}

fn run_check(core: &FakeCore, step: u32, reg: &[u8]) -> i32 {
    unsafe { <Machines as Service<FakeCore>>::check(core, step, &check_ctx(reg)) }
}

fn apply_ctx(reg: &[u8]) -> ApplyCtx {
    ApplyCtx {
        owner: OWNER,
        reg: reg.as_ptr(),
        base: BASE,
    }
}

/// Press and release of encoder `enc` on `VIEW`, with the page answering that `code` is under it.
fn click(core: &FakeCore, code: i32, track: u32, enc: u32) -> slots::Pending {
    let fw = FakeFw {
        param_code: code,
        track,
        ..FakeFw::default()
    };
    let event = |flags: u32| [0u32, 0, 0, 0, flags]; // the key code at +0xc, the flags at +0x10
    let mut press = event(slots::F_PRESS);
    let mut release = event(slots::F_RELEASE);
    press[3] = slots::FIRST_CODE + enc;
    release[3] = slots::FIRST_CODE + enc;
    unsafe {
        // the view's track reference word (`view + 0x74`) is read before the page is asked: a block of words stands in for the view
        let view = [0u32; 0x40];
        let v = view.as_ptr() as *const u8;
        slots::page_key(core, &fw, v, press.as_ptr() as *const u8);
        slots::page_key(core, &fw, v, release.as_ptr() as *const u8);
        slots::pending()
    }
}

#[test]
fn the_records_of_a_good_section_pass_every_check_step() {
    let core = core();
    let reg = section(&[
        param(0, SLOT_BASE, 0, 255, 0, "LEVEL"),
        param(1, SLOT_BASE + 1, -64, 63, 0, "PAN"),
        action(1, 8),
    ]);
    for step in [
        check_step::COUNTS,
        check_step::ACTIONS,
        check_step::NATIVE,
        check_step::DEVICE,
        check_step::RECORDS,
        check_step::OWNERS,
        check_step::ROWS,
    ] {
        assert_eq!(run_check(&core, step, &reg), OK, "step {step}");
    }
}

#[test]
fn a_bad_action_a_bad_parameter_or_a_page_without_page_rows_is_refused() {
    let core = core();
    // an action for a parameter the file does not have, and two actions for one parameter
    let reg = section(&[param(0, SLOT_BASE, 0, 255, 0, "LEVEL"), action(1, 8)]);
    assert_eq!(run_check(&core, check_step::ACTIONS, &reg), E_FORMAT);
    let reg = section(&[
        param(0, SLOT_BASE, 0, 255, 0, "LEVEL"),
        action(0, 8),
        action(0, 12),
    ]);
    // the rule of the validator (`validate::Acc`), which the COUNTS step runs
    assert_eq!(run_check(&core, check_step::COUNTS, &reg), E_FORMAT);
    // a callback outside the image
    let reg = section(&[param(0, SLOT_BASE, 0, 255, 0, "LEVEL"), action(0, 0x100)]);
    assert_eq!(run_check(&core, check_step::ACTIONS, &reg), E_FORMAT);
    // a slot below the first knob slot (the device's), a range upside down and an index out of order (the validator's, run by COUNTS)
    for (step, bad) in [
        (
            check_step::RECORDS,
            param(0, SLOT_BASE - 1, 0, 255, 0, "LEVEL"),
        ),
        (check_step::COUNTS, param(0, SLOT_BASE, 9, 1, 0, "LEVEL")),
        (check_step::COUNTS, param(2, SLOT_BASE, 0, 1, 0, "LEVEL")),
    ] {
        assert_eq!(run_check(&core, step, &section(&[bad])), E_FORMAT);
    }
    // pages need a device that has page rows (the table has none)
    let reg = section(&[(KIND_PAGE, std::vec![0u8; LEN_PAGE as usize - 8])]);
    assert_eq!(run_check(&core, check_step::DEVICE, &reg), E_FORMAT);
}

#[test]
fn a_table_with_more_rows_than_the_state_has_or_no_table_refuses_the_records_that_need_it() {
    let reg = section(&[param(0, SLOT_BASE, 0, 255, 17, "LEVEL")]);
    let mut core = FakeCore::new();
    // an image without `dynreg_info`: the section needs a registry, one without machine records does not
    assert_eq!(run_check(&core, check_step::COUNTS, &reg), E_NO_REGISTRY);
    assert_eq!(run_check(&core, check_step::COUNTS, &section(&[])), OK);
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::apply(&core, &apply_ctx(&reg)) },
        E_NO_REGISTRY
    );
    // a table that fits
    core.set_dynreg(table());
    assert_eq!(run_check(&core, check_step::COUNTS, &reg), OK);
    // a table that says more parameter rows than the state has
    let mut inf = table();
    inf[w::P_DYN_N] = limits::PARAM_ROWS as u32 + 1;
    core.set_dynreg(inf);
    assert_eq!(run_check(&core, check_step::COUNTS, &reg), E_NO_REGISTRY);
}

#[test]
fn apply_fills_the_parameter_rows_and_arms_the_click_and_remove_gives_them_back() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut core = core();
    let inf = table();
    let reg = section(&[
        param(0, SLOT_BASE, 0, 255, 17, "LEVEL"),
        param(1, SLOT_BASE + 1, -64, 63, 0, "PAN"),
        action(1, 8),
    ]);
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::apply(&core, &apply_ctx(&reg)) },
        OK
    );
    // the parameters sit in the first free rows, with their ranges and the owner
    assert_eq!(row_words(&core, P_DYN0), [0, 255, 17]);
    assert_eq!(row_words(&core, P_DYN0 + 1), [-64i32 as u32, 63, 0]);
    assert_eq!(&rstate().param_owner[..3], [OWNER, OWNER, 0]);
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::cap(&core, dkrt_sdk::proto::CAPK_PARAM_ROWS) },
        Some((16, 2))
    );
    // the first parameter has no action: the stock click, nothing is queued
    assert_eq!(&rstate().param_owner[..3], [OWNER, OWNER, 0]);
    assert_eq!(click(&core, P_DYN0 as i32, 6, 1).callback, 0);
    // a click on the second parameter queues the callback at the module's load address plus its image offset
    let p = click(&core, (P_DYN0 + 1) as i32, 6, 0);
    assert_eq!((p.callback, p.track, p.code), (BASE + 8, 6, P_DYN0 + 1));
    assert_eq!(
        slots::click_context(&core, &p).map(|c| c.owner),
        Some(OWNER)
    );
    // remove: the rows are free, the action is disarmed and the queued click of the unloaded module is dropped
    core.calls.borrow_mut().clear();
    unsafe { <Machines as Service<FakeCore>>::remove(&core, OWNER) };
    assert_eq!(&rstate().param_owner[..3], [0, 0, 0]);
    assert_eq!(
        (row_words(&core, P_DYN0), row_words(&core, P_DYN0 + 1)),
        ([0, 0, 0], [0, 0, 0]),
        "both parameter rows are written back as gap rows"
    );
    assert_eq!(unsafe { slots::pending() }.callback, 0);
    let p = click(&core, (P_DYN0 + 1) as i32, 6, 2);
    assert_eq!(p.callback, 0, "no action is armed any more");
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::cap(&core, dkrt_sdk::proto::CAPK_PARAM_ROWS) },
        Some((16, 0))
    );
}

#[test]
fn a_native_page_record_registers_its_callbacks_through_the_image_and_other_kinds_do_nothing() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = core();
    let inf = table();
    // the key of the machine is a pinned type number of the run-time range
    let (kind, body) = native(0x12);
    let reg = section(&[(kind, body.clone())]);
    let ctx = apply_ctx(&reg);
    // what the adapter's `mf_native_register` reads, taken while its arguments are valid: (owner, draw, event, snapshot words, arg, with_arg)
    let seen: Rc<RefCell<Vec<(usize, usize, usize, Vec<u32>, usize, usize)>>> =
        Rc::new(RefCell::new(Vec::new()));
    let sink = seen.clone();
    *core.on_call.borrow_mut() = Some(Box::new(move |addr, a| {
        assert_eq!(addr, N_REGISTER, "version 1 records use the v1 function");
        let words = unsafe { std::slice::from_raw_parts(a[5] as *const u32, a[6]) }.to_vec();
        sink.borrow_mut()
            .push((a[0], a[2], a[3], words, a[7], a[8]));
        0
    }));
    unsafe {
        assert_eq!(
            <Machines as Service<FakeCore>>::apply_record(&core, KIND_PARAM, body.as_ptr(), &ctx),
            OK
        );
        assert!(
            core.calls().is_empty(),
            "a kind of another service or a parameter record writes nothing"
        );
        assert_eq!(
            <Machines as Service<FakeCore>>::apply_record(
                &core,
                KIND_NATIVE_PAGE,
                body.as_ptr(),
                &ctx
            ),
            OK
        );
    }
    assert_eq!(
        *seen.borrow(),
        [(0x12, BASE as usize + 2, BASE as usize + 4, Vec::new(), 0, 0)]
    );
    // a refusal of the firmware adapter is the record's status
    *core.on_call.borrow_mut() = Some(Box::new(|_, _| E_NO_REGISTRY as usize));
    assert_eq!(
        unsafe {
            <Machines as Service<FakeCore>>::apply_record(
                &core,
                KIND_NATIVE_PAGE,
                body.as_ptr(),
                &ctx,
            )
        },
        E_NO_REGISTRY
    );
    // a turn calls the adapter's turn function, and its answer says whether the turn was consumed
    *core.on_call.borrow_mut() = None;
    core.fw_result.set(1);
    assert_eq!(
        unsafe { dkrt_svc_machines::nativepage::turn(&core, std::ptr::null(), std::ptr::null()) },
        1
    );
    // a firmware without the native page calls (the words are 0) refuses the record when it is checked, registers nothing and consumes no turn
    let mut bare = core;
    bare.info_tables
        .retain(|(h, _)| *h != name_hash("machines_info"));
    bare.info_tables
        .push((name_hash("machines_info"), machines_info(false)));
    assert_eq!(
        run_check(&bare, check_step::NATIVE, &section(&[(kind, body.clone())])),
        E_NO_REGISTRY
    );
    bare.calls.borrow_mut().clear();
    assert_eq!(
        unsafe {
            <Machines as Service<FakeCore>>::apply_record(
                &bare,
                KIND_NATIVE_PAGE,
                body.as_ptr(),
                &ctx,
            )
        },
        OK,
        "an unbound word is not called (the check refused the record before; apply has nothing to register)"
    );
    assert_eq!(
        unsafe { dkrt_svc_machines::nativepage::turn(&bare, std::ptr::null(), std::ptr::null()) },
        0
    );
    assert!(bare.calls().iter().all(|c| !matches!(c, Call::Fw { .. })));
}

#[test]
fn the_capacity_counts_need_a_registry_and_the_ui_pass_runs_the_click_last() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut core = FakeCore::new();
    rstate().mach_owner = [0; limits::MACHINE_ROWS]; // other tests leave rows owned in the static state
    for kind in [
        dkrt_sdk::proto::CAPK_MACHINE_ROWS,
        dkrt_sdk::proto::CAPK_PARAM_ROWS,
        dkrt_sdk::proto::CAPK_PAGE_ROWS,
    ] {
        assert_eq!(
            unsafe { <Machines as Service<FakeCore>>::cap(&core, kind) },
            Some((0, 0)),
            "no registry"
        );
    }
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::cap(&core, 0xff) },
        None,
        "a kind another service counts"
    );
    core.set_dynreg(table());
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::cap(&core, dkrt_sdk::proto::CAPK_MACHINE_ROWS) },
        Some((8, 0))
    );
    assert_eq!(
        unsafe { <Machines as Service<FakeCore>>::cap(&core, dkrt_sdk::proto::CAPK_PAGE_ROWS) },
        Some((4, 0))
    );
    // no click is queued: the pass in any phase runs nothing
    unsafe { <Machines as Service<FakeCore>>::tick(&core, tick_phase::LAST) };
}

#[test]
fn the_machine_types_of_a_module_are_its_machine_rows_in_type_order() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = core();
    let s = rstate();
    s.mach_owner[1] = 4;
    s.mach_owner[5] = 4;
    s.mach_owner[2] = 9;
    let mut out = [0u8; limits::MACHINE_ROWS];
    let first = dkrt_abi::profiles::DT2.dyn_type_first as u8;
    assert_eq!(types_of(&core, 4, &mut out), 2);
    assert_eq!(&out[..2], [first + 1, first + 5]);
    assert_eq!(types_of(&core, 7, &mut out), 0);
    s.mach_owner = [0; limits::MACHINE_ROWS];
}

#[test]
fn the_machines_of_a_module_are_its_rows_its_types_and_the_guard_counts_for_them() {
    // TEST_LOCK first (this file's statics, as its neighbours), then the contract lock (the service row statics)
    let _tl = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _lock = dkrt_test_support::contract::locked();
    let core = core();
    dkrt_svc_machines::rows::reset();
    let first = dkrt_abi::profiles::DT2.dyn_type_first;
    // module record 1 (owner 2) has a machine row (type first + 3) and a voice row of another type (a DSP-only machine) and one of the type of its machine row
    rstate().mach_owner[3] = 2;
    let v = dkrt_svc_machines::rows::vrows();
    for (k, t) in [(0usize, first + 3), (1, 0x1e)] {
        v.row_owner[k] = 2;
        v.row_type[k] = t;
    }
    v.mod_rows[1] = 0b11;
    let m = dkrt_svc_machines::records::module_info(&core, 2);
    assert_eq!(
        (m.rows, m.nt, m.types[0], m.types[1]),
        (0b11, 2, first as u8 + 3, 0x1e),
        "the machine row's type, then the voice row's type that no machine row names (the shared type once)"
    );
    // a module with neither
    assert_eq!(
        dkrt_svc_machines::records::module_info(&core, 5),
        Default::default()
    );
    // the guard's counts of its types
    let mut w = [0u32; dkrt_sdk::fault::DIAG_WORDS];
    w[17] = 3 | 1 << 24;
    w[18] = 0x1e;
    w[23] = 0x1e;
    unsafe { <Machines as Service<FakeCore>>::diag(&core, &w) };
    let m = dkrt_svc_machines::records::module_info(&core, 2);
    assert_eq!((m.faults, m.guard_off), (3, 1));
    // at most 8 types
    for k in 0..8 {
        v.row_owner[k] = 2;
        v.row_type[k] = 0x40 + k as u32;
    }
    assert_eq!(dkrt_svc_machines::records::module_info(&core, 2).nt, 8);
    rstate().mach_owner[3] = 0;
    dkrt_svc_machines::rows::reset();
}

#[test]
fn apply_refuses_a_machine_without_a_run_time_type_number_instead_of_indexing_the_rows() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut inf = table();
    inf[w::M_FIRST] = 7;
    inf[w::M_DYN0] = 16;
    inf[w::M_DYN_N] = 8;
    let mut core = core();
    core.set_dynreg(inf);
    // a machine key that `assign` has no number for (nothing was planned for the module being loaded: the boot index is not the module's), and a pinned number below the range
    for key in [0x1100u32, 0x5] {
        let mut b = std::vec![0u8; LEN_MACHINE as usize - 8];
        put(&mut b, 0, key);
        put(&mut b, 4, 0);
        put(&mut b, 8, 255);
        let reg = section(&[param(0, SLOT_BASE, 0, 255, 0, "LEVEL"), (KIND_MACHINE, b)]);
        assert_eq!(
            unsafe { <Machines as Service<FakeCore>>::apply(&core, &apply_ctx(&reg)) },
            E_TYPE_RANGE,
            "key {key:#x}"
        );
        assert_eq!(&rstate().param_owner[..2], [0, 0], "nothing was written");
    }
}
