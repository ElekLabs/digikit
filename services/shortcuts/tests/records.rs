//! Host tests of the shortcuts service over `FakeCore`: the builder call and its commit, the rule for the records, `check`, `apply_record` and `remove`, and the entry for the
//! key-dispatch hook. The statics of the crate (the collection, the gate and the state) are shared, so each test takes `LOCK` and starts clean.
use core::ffi::c_void;
use dkrt_abi::profiles::ALL;
use dkrt_abi::service::{
    check_step, commit_phase, ApplyCtx, CheckCtx, Declare, ModuleCommitCtx, RecordSink, Service,
};
use dkrt_sdk::records::{KIND_SHORTCUT, LEN_SHORTCUT};
use dkrt_sdk::*;
use dkrt_svc_shortcuts::bindings::Callback;
use dkrt_svc_shortcuts::build::{begin, end, shortcut_add, ShortcutDef};
use dkrt_svc_shortcuts::records::{on_key, state, ShortcutsService};
use dkrt_svc_shortcuts::shortcuts::Verdict;
use dkrt_svc_shortcuts::validate::{at, check_set, BODY_LEN};
use dkrt_test_support::{Call, FakeCore};
use std::sync::{Mutex, MutexGuard};

static LOCK: Mutex<()> = Mutex::new(());
type Svc = ShortcutsService;

const BASE: u32 = 0x4000;
const IMAGE: u32 = 0x1000;
/// The logical keys of these tests: the generated numbers of `[keys] order` (sdk/sdk.toml).
use dkrt_sdk::keys::{NO as OTHER, TRK, YES};

/// An address of the module image that stands for a callback (never called here).
fn code(offset: u32) -> Option<Callback> {
    // SAFETY: a fake address, only compared and stored
    Some(unsafe { core::mem::transmute::<usize, Callback>((BASE + offset) as usize) })
}

fn def(
    id: &'static [u8],
    label: &'static [u8],
    keys: [u32; 2],
    on: Option<Callback>,
    off: Option<Callback>,
) -> ShortcutDef {
    ShortcutDef {
        id: if id.is_empty() {
            core::ptr::null()
        } else {
            id.as_ptr()
        },
        label: label.as_ptr(),
        keys,
        on,
        off,
        arg: 0x77 as *mut c_void,
    }
}

fn hint() -> ShortcutDef {
    def(
        b"hint\0",
        b"TRACK NAME HINT\0",
        [TRK, YES],
        code(0x100),
        code(0x110),
    )
}

fn fresh() -> (MutexGuard<'static, ()>, FakeCore) {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut core = FakeCore::new();
    // a device that has the keys of these tests
    core.device = ALL
        .iter()
        .find(|p| [TRK, YES, OTHER].iter().all(|k| p.key_code(*k) != 0))
        .expect("a profile with key names")
        .id;
    for owner in 0..=8u8 {
        unsafe { <Svc as Service<FakeCore>>::remove(&core, owner) };
    }
    assert_eq!(state().table().len(), 0);
    (g, core)
}

/// `init` of module record `rec` is open and its code runs.
fn open(core: &FakeCore, rec: usize) {
    begin(rec);
    core.running.set(rec as u8 + 1);
}

fn add(core: &FakeCore, d: &ShortcutDef) -> i32 {
    unsafe { shortcut_add(core, 4 as *mut u8, d as *const ShortcutDef as *const u8) }
}

fn ctx(phase: u32, sink: *mut RecordSink) -> ModuleCommitCtx<'static> {
    static KEYS: dkrt_abi::keys::KeysCopy = dkrt_abi::keys::KeysCopy::empty();
    ModuleCommitCtx {
        phase,
        base: BASE,
        image_size: IMAGE,
        keys: &KEYS,
        sink,
    }
}

/// The commit of module record `rec`: the first refusal, or the bytes of the records it wrote.
fn commit(core: &FakeCore, rec: usize) -> Result<Vec<u8>, i32> {
    let mut measure = RecordSink::measure();
    let r = unsafe {
        <Svc as Service<FakeCore>>::module_commit(
            core,
            rec,
            &ctx(commit_phase::PREPARE, &mut measure),
        )
    };
    if r != OK {
        return Err(r);
    }
    let mut buf = vec![0u8; measure.at];
    let mut sink = RecordSink::over(buf.as_mut_ptr(), buf.len());
    let r = unsafe {
        <Svc as Service<FakeCore>>::module_commit(core, rec, &ctx(commit_phase::WRITE, &mut sink))
    };
    assert_eq!(r, OK);
    assert_eq!(
        sink.at, measure.at,
        "the measuring and the writing pass agree"
    );
    unsafe {
        <Svc as Service<FakeCore>>::module_commit(core, rec, &ctx(commit_phase::DECLARE, &mut sink))
    };
    Ok(buf[RecordSink::HEADER..].to_vec())
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[test]
fn the_service_declares_the_shortcut_kind() {
    let mut d = Declare::new();
    unsafe { <Svc as Service<FakeCore>>::declare(&mut d) };
    assert_eq!(d.n, 1);
}

#[test]
fn a_call_outside_init_or_from_another_module_is_refused() {
    let (_g, core) = fresh();
    end();
    assert_eq!(add(&core, &hint()), E_CONTEXT, "no init is open");
    begin(0);
    core.running.set(2);
    assert_eq!(add(&core, &hint()), E_CONTEXT, "another module's code runs");
    end();
}

#[test]
fn the_commit_writes_the_record_with_the_callbacks_as_image_offsets() {
    let (_g, core) = fresh();
    open(&core, 0);
    assert_eq!(add(&core, &hint()), OK);
    let bytes = commit(&core, 0).unwrap();
    assert_eq!(bytes.len(), LEN_SHORTCUT as usize);
    assert_eq!(
        (word(&bytes, 0), word(&bytes, 4)),
        (KIND_SHORTCUT, LEN_SHORTCUT)
    );
    let b = &bytes[8..];
    assert_eq!(&b[at::ID..at::ID + 5], b"hint\0");
    assert_eq!(&b[at::LABEL..at::LABEL + 16], b"TRACK NAME HINT\0");
    assert_eq!((word(b, at::KEY0), word(b, at::KEY1)), (TRK, YES));
    assert_eq!(
        (word(b, at::ON), word(b, at::OFF), word(b, at::ARG)),
        (0x100, 0x110, 0x77)
    );
}

#[test]
fn a_shortcut_without_off_has_the_offset_zero() {
    let (_g, core) = fresh();
    open(&core, 0);
    assert_eq!(
        add(&core, &def(b"x\0", b"X\0", [TRK, YES], code(0x100), None)),
        OK
    );
    let bytes = commit(&core, 0).unwrap();
    assert_eq!(word(&bytes[8..], at::OFF), 0);
}

#[test]
fn a_module_without_shortcuts_commits_nothing() {
    let (_g, core) = fresh();
    open(&core, 0);
    assert_eq!(commit(&core, 0).unwrap().len(), 0);
}

#[test]
fn the_commit_refuses_a_bad_call_with_the_first_error() {
    // a fifth shortcut
    let (_g, core) = fresh();
    open(&core, 0);
    for k in 0..4u32 {
        assert_eq!(
            add(
                &core,
                &def(b"a\0", b"A\0", [TRK, 10 + k], code(0x100), None)
            ),
            OK
        );
    }
    assert_eq!(add(&core, &hint()), E_DECL_FULL);
    assert_eq!(commit(&core, 0), Err(E_DECL_FULL));
    // no id, no struct
    open(&core, 0);
    assert_eq!(
        add(&core, &def(b"", b"A\0", [TRK, YES], code(0x100), None)),
        E_FORMAT
    );
    assert_eq!(commit(&core, 0), Err(E_FORMAT));
    open(&core, 0);
    assert_eq!(
        unsafe { shortcut_add(&core, 4 as *mut u8, core::ptr::null()) },
        E_FORMAT
    );
    assert_eq!(commit(&core, 0), Err(E_FORMAT));
    // the rule of `validate`: no `on`, a key that is NONE, the same key twice, the same keys twice, an id twice
    for (what, bad) in [
        ("no on", def(b"a\0", b"A\0", [TRK, YES], None, None)),
        ("key none", def(b"a\0", b"A\0", [0, YES], code(0x100), None)),
        (
            "same key",
            def(b"a\0", b"A\0", [TRK, TRK], code(0x100), None),
        ),
        (
            "key past the table",
            def(b"a\0", b"A\0", [TRK, 64], code(0x100), None),
        ),
    ] {
        open(&core, 0);
        assert_eq!(
            add(&core, &bad),
            OK,
            "{what}: the call collects, the commit judges"
        );
        assert_eq!(commit(&core, 0), Err(E_FORMAT), "{what}");
    }
    open(&core, 0);
    add(&core, &def(b"a\0", b"A\0", [TRK, YES], code(0x100), None));
    add(&core, &def(b"b\0", b"B\0", [TRK, YES], code(0x100), None));
    assert_eq!(commit(&core, 0), Err(E_FORMAT), "same keys twice");
    open(&core, 0);
    add(&core, &def(b"a\0", b"A\0", [TRK, YES], code(0x100), None));
    add(&core, &def(b"a\0", b"B\0", [TRK, OTHER], code(0x100), None));
    assert_eq!(commit(&core, 0), Err(E_FORMAT), "same id twice");
    // a callback outside the image
    open(&core, 0);
    add(&core, &def(b"a\0", b"A\0", [TRK, YES], code(IMAGE), None));
    assert_eq!(commit(&core, 0), Err(E_FORMAT));
    end();
}

/// A `register` section of the records the commit wrote.
fn section(records: &[u8], n: u32) -> Vec<u8> {
    let mut s = vec![0u8; 16];
    s[12..16].copy_from_slice(&n.to_be_bytes());
    s.extend_from_slice(records);
    s
}

fn check(core: &FakeCore, step: u32, sec: &[u8]) -> i32 {
    let c = CheckCtx {
        reg: sec.as_ptr(),
        image_size: IMAGE,
    };
    unsafe { <Svc as Service<FakeCore>>::check(core, step, &c) }
}

fn apply(core: &FakeCore, owner: u8, records: &[u8]) -> i32 {
    let c = ApplyCtx {
        owner,
        reg: core::ptr::null(),
        base: BASE,
    };
    unsafe {
        <Svc as Service<FakeCore>>::apply_record(core, KIND_SHORTCUT, records[8..].as_ptr(), &c)
    }
}

fn one_record(core: &FakeCore, d: &ShortcutDef) -> Vec<u8> {
    open(core, 0);
    assert_eq!(add(core, d), OK);
    commit(core, 0).unwrap()
}

#[test]
fn apply_makes_a_binding_with_the_callbacks_resolved_against_the_base_and_remove_drops_it() {
    let (_g, core) = fresh();
    let rec = one_record(&core, &hint());
    assert_eq!(check(&core, check_step::RECORDS, &section(&rec, 1)), OK);
    assert_eq!(check(&core, check_step::ROWS, &section(&rec, 1)), OK);
    assert_eq!(apply(&core, 1, &rec), OK);
    let b = state().table().get(0).unwrap();
    assert_eq!(
        (b.owner, b.keys, b.disabled),
        (1, [TRK as u8, YES as u8], false)
    );
    assert_eq!(
        b.on.map(|f| f as *const () as usize),
        Some((BASE + 0x100) as usize)
    );
    assert_eq!(
        b.off.map(|f| f as *const () as usize),
        Some((BASE + 0x110) as usize)
    );
    assert_eq!(b.arg as usize, 0x77);
    assert_eq!(&b.id[..5], b"hint\0");
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 1) };
    assert_eq!(state().table().len(), 0);
}

#[test]
fn a_second_module_with_the_same_keys_loads_with_its_shortcut_disabled_and_a_log_entry() {
    let (_g, core) = fresh();
    let rec = one_record(&core, &hint());
    assert_eq!(apply(&core, 1, &rec), OK);
    assert!(core.calls().iter().all(|c| !matches!(c, Call::Log { .. })));
    assert_eq!(apply(&core, 2, &rec), OK, "a clash is no refusal");
    let logs: Vec<_> = core
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Log { kind, args, .. } => Some((kind, args)),
            _ => None,
        })
        .collect();
    assert_eq!(logs, [(logk::SHORTCUT_CLASH, [2, 1, TRK, YES, 0])]);
    assert!(state().table().get(1).unwrap().disabled);
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 1) };
    assert_eq!(state().table().len(), 1);
    assert!(
        !state().table().get(1).unwrap().disabled,
        "the second module's shortcut works once the first module is gone"
    );
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 2) };
}

#[test]
fn check_refuses_a_record_that_breaks_the_rule_or_points_outside_the_image_and_a_full_table() {
    let (_g, core) = fresh();
    let mut rec = one_record(&core, &hint());
    let mut bad = rec.clone();
    bad[8 + at::ON..8 + at::ON + 4].copy_from_slice(&IMAGE.to_be_bytes());
    assert_eq!(
        check(&core, check_step::RECORDS, &section(&bad, 1)),
        E_FORMAT
    );
    let mut bad = rec.clone();
    bad[8 + at::KEY0..8 + at::KEY0 + 4].copy_from_slice(&0u32.to_be_bytes());
    assert_eq!(
        check(&core, check_step::RECORDS, &section(&bad, 1)),
        E_FORMAT
    );
    // the table is full: 16 bindings of other owners
    for i in 0..16u32 {
        let mut r = rec.clone();
        r[8 + at::KEY1..8 + at::KEY1 + 4].copy_from_slice(&(10 + i).to_be_bytes());
        assert_eq!(apply(&core, 5, &r), OK);
    }
    assert_eq!(
        check(&core, check_step::ROWS, &section(&rec, 1)),
        E_TABLE_FULL
    );
    assert_eq!(apply(&core, 6, &rec), E_TABLE_FULL);
    // while a reload of that owner gives its bindings back first
    core.live_op.set(5);
    assert_eq!(check(&core, check_step::ROWS, &section(&rec, 1)), OK);
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 5) };
    rec.clear();
}

#[test]
fn a_record_of_another_kind_is_ignored() {
    let (_g, core) = fresh();
    let c = ApplyCtx {
        owner: 1,
        reg: core::ptr::null(),
        base: BASE,
    };
    assert_eq!(
        unsafe {
            <Svc as Service<FakeCore>>::apply_record(
                &core,
                KIND_SHORTCUT + 1,
                [0u8; 60].as_ptr(),
                &c,
            )
        },
        OK
    );
    assert_eq!(state().table().len(), 0);
}

#[test]
fn the_rule_reports_each_fault_of_a_record_and_judges_only_this_kind() {
    let good = {
        let mut b = vec![0u8; BODY_LEN];
        b[at::ID..at::ID + 5].copy_from_slice(b"hint\0");
        b[at::LABEL..at::LABEL + 2].copy_from_slice(b"X\0");
        b[at::KEY0 + 3] = TRK as u8;
        b[at::KEY1 + 3] = YES as u8;
        b[at::ON + 3] = 0x40;
        b
    };
    let faults = |bodies: &[&[u8]]| {
        let mut n = 0;
        check_set(bodies, &mut |_| n += 1);
        n
    };
    assert_eq!(faults(&[&good]), 0);
    assert_eq!(faults(&[]), 0);
    assert_eq!(faults(&[&good[..BODY_LEN - 1]]), 1, "a body one byte short");
    let mut bad = good.clone();
    bad[at::ID] = b' ';
    assert_eq!(faults(&[&bad]), 1, "a space in an id");
    let mut bad = good.clone();
    bad[at::LABEL..at::LABEL + 24].copy_from_slice(&[b'L'; 24]);
    assert_eq!(faults(&[&bad]), 1, "a label with no room for the NUL");
    // other kinds are not this service's to judge
    let mut n = 0;
    dkrt_svc_shortcuts::records::validate(KIND_SHORTCUT + 1, &[&[0u8; 3]], &mut |_| n += 1);
    assert_eq!(n, 0);
}

#[test]
fn the_dispatch_entry_translates_the_panel_code_with_the_device_profile() {
    let (_g, mut core) = fresh();
    let p = ALL
        .iter()
        .find(|p| p.key_code(TRK) != 0 && p.key_code(YES) != 0);
    // the device facts hold the key names: the profile of the device the core runs as maps TRK and YES (they are numbers of the generated keys.h, here 48 and 50)
    let p = p.copied().unwrap_or(ALL[0]);
    core.device = p.id;
    let (trk, yes) = (
        p.key_codes.iter().position(|c| *c == 16).unwrap_or(0) as u32,
        p.key_codes.iter().position(|c| *c == 10).unwrap_or(0) as u32,
    );
    assert!(trk != 0 && yes != 0);
    static CALLS: Mutex<Vec<&str>> = Mutex::new(Vec::new());
    unsafe extern "C" fn on(_: *mut c_void) {
        CALLS.lock().unwrap().push("on");
    }
    unsafe extern "C" fn off(_: *mut c_void) {
        CALLS.lock().unwrap().push("off");
    }
    let mut b = dkrt_svc_shortcuts::bindings::Binding::free();
    b.owner = 1;
    b.keys = [trk as u8, yes as u8];
    b.on = Some(on);
    b.off = Some(off);
    state().table_mut().add(b);
    let pass = |phase: u32| unsafe { <Svc as Service<FakeCore>>::tick(&core, phase) };
    // the firmware's flags word, read with the device's bits
    let (down, up) = (1u32 << p.key_flags.press, 1u32 << p.key_flags.release);
    assert_eq!(on_key(&core, p.key_code(trk), down), Verdict::Pass);
    assert_eq!(on_key(&core, p.key_code(yes), down), Verdict::Swallow);
    assert!(
        CALLS.lock().unwrap().is_empty(),
        "the hook runs no module code"
    );
    // the UI pass makes the calls (the early phase)
    pass(dkrt_abi::service::tick_phase::LAST);
    assert!(CALLS.lock().unwrap().is_empty());
    pass(dkrt_abi::service::tick_phase::EARLY);
    assert_eq!(*CALLS.lock().unwrap(), ["on"]);
    assert_eq!(on_key(&core, p.key_code(yes), up), Verdict::Swallow);
    assert_eq!(on_key(&core, p.key_code(trk), up), Verdict::Pass);
    pass(dkrt_abi::service::tick_phase::EARLY);
    assert_eq!(*CALLS.lock().unwrap(), ["on", "off"]);
    // a device the table does not know passes everything
    core.device = 999;
    assert_eq!(on_key(&core, p.key_code(yes), down), Verdict::Pass);
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 1) };
}

#[test]
fn a_binding_with_a_key_the_device_lacks_is_disabled_and_never_keeps_the_pair() {
    let (_g, mut core) = fresh();
    let rec = one_record(&core, &hint());
    // a device with no key names: the keys have no panel code
    let bare = ALL
        .iter()
        .find(|p| p.key_code(TRK) == 0)
        .expect("a profile without key names");
    core.device = bare.id;
    assert_eq!(apply(&core, 1, &rec), OK, "the module loads");
    let b = state().table().get(0).unwrap();
    assert!(b.disabled && b.inactive);
    let logs = |core: &FakeCore| -> Vec<(u16, [u32; 5])> {
        core.calls()
            .into_iter()
            .filter_map(|c| match c {
                Call::Log { kind, args, .. } => Some((kind, args)),
                _ => None,
            })
            .collect()
    };
    assert_eq!(
        logs(&core),
        [(logk::SHORTCUT_INACTIVE, [1, TRK, YES, 0, 0])]
    );
    // a device that has the keys: a later binding of the same pair is the keeper, not a clash with the inactive one
    let full = ALL
        .iter()
        .find(|p| p.key_code(TRK) != 0 && p.key_code(YES) != 0)
        .expect("a profile with key names");
    core.device = full.id;
    assert_eq!(apply(&core, 2, &rec), OK);
    let t = state().table();
    assert!(!t.get(1).unwrap().disabled, "the later binding works");
    assert!(
        logs(&core).iter().all(|l| l.0 != logk::SHORTCUT_CLASH),
        "no clash is logged for the inactive binding"
    );
    // removing the keeper does not enable the inactive binding
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 2) };
    assert!(state().table().get(0).unwrap().disabled);
    unsafe { <Svc as Service<FakeCore>>::remove(&core, 1) };
}
