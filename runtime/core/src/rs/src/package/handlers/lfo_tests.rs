//! Host tests of the `lfo` service through the runtime (the crate `dkrt-svc-lfo` has its own over `FakeCore`): the proof that a kind the registry, the collector, the loader, dkneeds and patchkit do not name works end to end
//! (R7): a fixture's record goes through `dk_record_add`'s collector, the `register` section, the registry walkers and the planner.
use super::exports::{owner_of, shape, used, MAX_SHAPES, SAMPLES};
use crate::package::collect::Collect;
use super::tests::LOCK;
use crate::package::admission::Admission;
use crate::package::registry;
use dkneeds::keys::{write_table, KeyGroup, Keys, Table};
use crate::package::core_api::RealCore;
use crate::package::svc_rows::{self, RowSource};
use dkrt_abi::plan::{Reason, Verdict};
use dkrt_abi::provider::Provider;
use dkrt_abi::service::{RecordSink, SettleCtx};
use dkrt_svc_lfo::lfo::Lfo;
use dkrt_svc_lfo_dks::{dk_svc_row, dk_svc_start, dk_svc_stop, DK_SVC_OWNER};
use dkrt_sdk::proto::*;
use dkrt_sdk::records::{KIND_LFO_SHAPE, KINDS_LFO, LEN_LFO_SHAPE};
use dkrt_sdk::*;
use std::vec;
use std::vec::Vec;

/// The body of a shape record: the key, the name, then the 32 samples (a ramp from `from`), big-endian.
fn body(id: u32, from: i16) -> Vec<u8> {
    let mut b = id.to_be_bytes().to_vec();
    // a name of its own: the service refuses two shapes of one module with names that look alike
    b.extend_from_slice(&[b'T', b'S', b'A' + (id >> 8) as u8 % 26, b'X']);
    for s in 0..SAMPLES as i16 {
        b.extend_from_slice(&(from + 100 * s).to_be_bytes());
    }
    b
}

/// What a fixture module's `init` does: `dk_record_add` for each shape; then the commit appends them to the `register` section.
fn section(shapes: &[(u32, i16)]) -> Vec<u8> {
    let mut c = Collect::new();
    for (id, from) in shapes {
        let b = body(*id, *from);
        assert_eq!(c.add_record(KIND_LFO_SHAPE, b.as_ptr(), b.len() as u32), OK);
    }
    // the commit appends the generic records to the block the services write, behind the header (magic, version, device, count)
    let mut sink = RecordSink::measure();
    c.write_generic(&mut sink);
    let mut buf = vec![0u8; sink.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    c.write_generic(&mut out);
    for (at, word) in [(0, 0x444b_5247u32), (4, 1), (8, 1), (12, out.count)] {
        buf[at..at + 4].copy_from_slice(&word.to_be_bytes());
    }
    buf
}

/// The lfo service as the runtime has it: a service file (`lfo.dks`), not a row of the image. The file's `init` makes the owner object (`dk_svc_start`), its row joins the
/// table as `Registry::add` does at the commit of the loaded file, and leaves it as `Registry::remove` does at the unload; `fini` stops the owner (`dk_svc_stop`).
pub(crate) struct Loaded;

/// The module record of the service file.
const FILE_REC: usize = 0;

impl Loaded {
    pub(crate) fn new() -> Loaded {
        let instance = dk_svc_start().expect("the owner starts");
        let row = RowSource::Provider { row: dk_svc_row(), instance, extent: 0..usize::MAX };
        assert_eq!(unsafe { svc_rows::add(FILE_REC, row, None) }, OK);
        Loaded
    }
}

impl Drop for Loaded {
    fn drop(&mut self) {
        unsafe { svc_rows::remove(FILE_REC) };
        dk_svc_stop();
    }
}

/// The boot's settle over one module (selection index 0) that provides these shape keys, and that module loading.
fn settle(keys: &[u32]) {
    crate::package::loader::test_accept(0, &table(&[(KIND_LFO_SHAPE, keys)]));
    let mut sel = SettleCtx::new();
    sel.count = 1;
    unsafe {
        DK_SVC_OWNER.with(|l| Provider::<RealCore>::settle(l, &crate::package::core_api::REAL_CORE, &sel)).expect("the owner is started");
        crate::package::boot_idx::set(0);
    }
}

/// The wave number the owner gave the key `key` of the module at selection index `at`.
fn number_of(at: u32, key: u32) -> u32 {
    DK_SVC_OWNER.with(|l: &mut Lfo| l.number_of(at, key)).expect("the owner is started")
}

fn clear() {
    for owner in 1..=dkrt_abi::MAX_MODULES as u8 {
        unsafe { registry::remove(owner) };
    }
}

#[test]
fn a_shape_record_goes_from_the_builder_call_to_the_table_and_back() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let _l = Loaded::new();
    clear();
    settle(&[0x700]);
    let sec = section(&[(0x700, -1000)]);
    // the section is the header and one record of the kind's length
    assert_eq!(sec.len(), 16 + LEN_LFO_SHAPE as usize);
    unsafe {
        assert_eq!(registry::check(sec.as_ptr(), sec.len() as u32, 0, 1), OK);
        assert_eq!(registry::apply(1, sec.as_ptr(), sec.len() as u32, 0x1000), OK);
    }
    let s = shape(number_of(0, 0x700)).expect("the walker applied the record");
    assert_eq!((s[0], s[1], s[31]), (-1000, -900, -1000 + 3100));
    assert_eq!((owner_of(number_of(0, 0x700)), used()), (1, 1));
    unsafe { registry::remove(1) };
    assert_eq!((shape(7), owner_of(7), used()), (None, 0, 0), "the walker removed what the module kept");
}

#[test]
fn an_id_twice_or_zero_and_a_file_that_does_not_fit_the_table_are_refused() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let _l = Loaded::new();
    clear();
    let keys: Vec<u32> = (1..=MAX_SHAPES as u32 + 1).map(|i| i << 8).collect();
    settle(&keys[..8]);
    let a = section(&[(0x100, 0)]);
    unsafe {
        assert_eq!(registry::check(a.as_ptr(), a.len() as u32, 0, 1), OK);
        assert_eq!(registry::apply(1, a.as_ptr(), a.len() as u32, 0x1000), OK);
        // another key is accepted (the same key twice is the planner's refusal, `E_PLAN_KEY`, before any check runs)
        let other = section(&[(0x200, 5)]);
        assert_eq!(registry::check(other.as_ptr(), other.len() as u32, 0, 1), OK);
        // an id twice in one file, and the id 0
        let twice = section(&[(0x300, 0), (0x300, 1)]);
        assert_eq!(registry::check(twice.as_ptr(), twice.len() as u32, 0, 1), E_TYPE_USED);
        let zero = section(&[(0, 0)]);
        assert_eq!(registry::check(zero.as_ptr(), zero.len() as u32, 0, 1), E_FORMAT);
        // a table that cannot take the file's shapes: one is kept, so all eight do not fit
        let many: Vec<(u32, i16)> = keys[..MAX_SHAPES].iter().map(|k| (*k, 0)).collect();
        let big = section(&many);
        assert_eq!(registry::check(big.as_ptr(), big.len() as u32, 0, 1), E_TABLE_FULL);
        registry::remove(1);
    }
    assert_eq!(used(), 0);
}

#[test]
fn a_record_of_a_kind_that_is_not_generic_or_has_the_wrong_length_never_reaches_the_service() {
    // `add_record` reads the service table, a static that the tests of the service rows replace under this lock
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let b = body(1, 0);
    assert_eq!(b.len() + 8, LEN_LFO_SHAPE as usize);
    let mut c = Collect::new();
    assert_eq!(c.add_record(KIND_LFO_SHAPE, b.as_ptr(), b.len() as u32 - 4), E_FORMAT);
    assert_eq!(c.ng, 0);
}

fn table(keys: &[(u32, &[u32])]) -> Vec<u8> {
    let groups: Vec<KeyGroup> = keys.iter().map(|(kind, ks)| KeyGroup { kind: *kind, keys: ks }).collect();
    let t = Table { keys: &groups, ..Table::default() };
    let mut b = vec![0u8; dkneeds::keys::table_size(&t)];
    write_table(&t, &mut b).unwrap();
    b
}

/// The verdicts of the rules of the core for `tables`, one module each, in selection order.
fn plan_of(tables: &[Vec<u8>]) -> Vec<Verdict> {
    let c = |kind: u8, limit: u32| DkrtCap { kind: kind as u32, limit, used: 0, largest: limit };
    let caps = [c(CAPK_MODULES, 8), c(CAPK_SELECTION, 8)];
    let keys: Vec<Option<Keys>> = tables.iter().map(|t| Some(Keys::parse(t).unwrap())).collect();
    let mut a = Admission::new(&caps);
    (0..keys.len()).map(|j| a.judge_module(&crate::package::handlers::services(), &keys, &[], j)).collect()
}

#[test]
fn admission_knows_the_kind_from_the_service_table_refuses_an_unowned_kind_and_a_second_owner() {
    let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    let _l = Loaded::new();
    let one = table(&[(KIND_LFO_SHAPE, &[7])]);
    let same = table(&[(KIND_LFO_SHAPE, &[7, 8])]);
    let other = table(&[(KIND_LFO_SHAPE, &[9])]);
    // the lfo service owns the kind: a module with a shape key is accepted
    let p = plan_of(&[one.clone()]);
    assert_eq!(p[0], Verdict::Accepted);
    // the same shape key in a second module is refused, another key is not
    let p = plan_of(&[one.clone(), same, other]);
    assert_eq!(p[..3], [Verdict::Accepted, Verdict::Refused(Reason::DuplicateKey(KIND_LFO_SHAPE, 7)), Verdict::Accepted]);
    assert_eq!(p[1].result(), E_PLAN_KEY);
    // a shape key twice in one module is refused too, so the assignment (`sticky::assign`) never sees a repeated key
    let twice = table(&[(KIND_LFO_SHAPE, &[5, 5])]);
    let p = plan_of(&[twice]);
    assert_eq!(p[0], Verdict::Refused(Reason::DuplicateKey(KIND_LFO_SHAPE, 5)));
    // a kind no service owns
    let p = plan_of(&[table(&[(77, &[1])])]);
    assert_eq!(p[0], Verdict::Refused(Reason::UnownedKind(77)));
    assert_eq!(p[0].result(), E_PLAN_KIND);
}
