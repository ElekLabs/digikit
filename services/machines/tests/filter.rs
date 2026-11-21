//! The filters of the machines service (docs/plans/extension-model.md E7): the builder calls and their refusals, the lowering to the FILTER record, the record rule, the
//! sticky numbers 16 to 31 and the service's check. The fixture `tests/fixtures/modules/filter-gain` makes the same calls as `gain()`.
use dkrt_abi::service::{Problem, RecordSink, Service, SettleCtx};
use dkrt_sdk::records::*;
use dkrt_sdk::*;
use dkrt_svc_machines::build::{self, *};
use dkrt_svc_machines::filter::{self, SETTING};
use dkrt_svc_machines::records::Machines;
use dkrt_test_support::FakeCore;
use std::boxed::Box;
use std::sync::Mutex;
use std::vec::Vec;

/// The tests drive the statics of the crate (the open collection, the filter table and map).
static LOCK: Mutex<()> = Mutex::new(());

fn c(s: &'static [u8]) -> *const u8 {
    assert_eq!(s.last(), Some(&0));
    s.as_ptr()
}

const KEY: u32 = 0x00ab_cd00;
const DT2: Profile = Profile {
    device: 1,
    pages: false,
    slot_base: 0,
    clone_of: 0,
    level: 10,
    own_dsp: false,
    tune_code: 0xca,
    tune_slot: 0,
    level_code: 0xd1,
    level_knob: 7,
    nslots: 6,
    slots: [1, 2, 4, 5, 6, 7, 0, 0],
    tpl: [0xcc, 0xd1, 0x47, 0x32, 0xca, 0xcb],
};

fn rd(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// The collection of `tests/fixtures/modules/filter-gain`: one filter, its page, the knob GAIN.
fn gain() -> Box<Build> {
    let mut b = Box::new(Build::new());
    let f = b.add_filter(0, c(b"Filter Gain\0"), c(b"FGAN\0"));
    let p = b.filter_add_page(f);
    b.page_add_param(p, c(b"Gain\0"), c(b"GAIN\0"), 64);
    assert_eq!(b.err, 0);
    b.filters[0].key = KEY; // the commit reads it from the keys table
    b
}

/// The records `lower` writes (header words not written) and how many.
fn lowered(b: &Build, base: u32) -> (Vec<u8>, u32) {
    let k = dkrt_abi::keys::KeysCopy::empty();
    let mut plan = Plan::new();
    let mut measured = RecordSink::measure();
    lower(b, &DT2, &k, &[true; MAX_M], base, &mut measured, &mut plan).unwrap();
    let mut buf = std::vec![0u8; measured.at];
    let mut out = RecordSink::over(buf.as_mut_ptr(), buf.len());
    lower(b, &DT2, &k, &[true; MAX_M], base, &mut out, &mut plan).unwrap();
    assert_eq!((out.at, out.count), (measured.at, measured.count));
    (buf[RecordSink::HEADER..].to_vec(), out.count) // the sink leaves room for the 16-byte section header
}

fn text(b: &[u8], at: usize, len: usize) -> &str {
    let t = &b[at..at + len];
    core::str::from_utf8(&t[..t.iter().position(|c| *c == 0).unwrap_or(len)]).unwrap()
}

#[test]
fn a_filter_lowers_to_one_record_with_its_key_names_and_own_knob() {
    let (buf, n) = lowered(&gain(), 0);
    assert_eq!(
        n, 1,
        "a filter adds a FILTER record and no parameter, machine or page record"
    );
    assert_eq!((rd(&buf, 0), rd(&buf, 4)), (KIND_FILTER, LEN_FILTER));
    assert_eq!(buf.len(), LEN_FILTER as usize);
    let body = &buf[8..];
    assert_eq!(
        (rd(body, 0), rd(body, 4)),
        (KEY, 1),
        "the key and one own knob"
    );
    assert_eq!(
        (text(body, 8, 24), text(body, 32, 24)),
        ("Filter Gain", "FGAN")
    );
    let k = &body[56..];
    // glyph encoder, 0 to 127, default 64 (raw = unit * 256)
    assert_eq!(
        (rd(k, 0), rd(k, 4), rd(k, 8), rd(k, 12)),
        (0, 0, 127 * 256, 64 * 256)
    );
    assert_eq!((text(k, 16, 32), text(k, 48, 16)), ("Gain", "GAIN"));
    assert!(
        body[56 + 64..].iter().all(|b| *b == 0),
        "the second slot is unused"
    );
}

#[test]
fn a_filter_knob_takes_a_parameter_row_and_the_filter_no_machine_or_page_row() {
    let b = gain();
    assert_eq!(counts_of(&b, &DT2), (0, 1, 0));
    let mut dn2 = DT2;
    dn2.pages = true;
    assert_eq!(counts_of(&b, &dn2), (0, 1, 0));
}

#[test]
fn a_third_knob_and_a_second_page_are_page_full() {
    let mut b = gain();
    let page = &mut b.pages[0] as *mut CPage as *mut u8;
    b.page_add_param(page, c(b"Two\0"), c(b"TWO\0"), 1);
    assert_eq!(b.err, 0, "knobs 2 and 3 are the module's");
    assert_eq!(b.nparam, 2);
    let third = b.page_add_param(page, c(b"Three\0"), c(b"THR\0"), 1);
    assert_eq!(
        (b.err, b.nparam),
        (E_PAGE_FULL, 2),
        "refused and not recorded"
    );
    assert_eq!(third, placeholder());

    let mut b = gain();
    let f = &mut b.filters[0] as *mut CFilter as *mut u8;
    assert_eq!(b.filter_add_page(f), placeholder());
    assert_eq!(b.err, E_PAGE_FULL);
}

#[test]
fn a_machine_handle_is_no_filter_and_a_filter_handle_is_no_machine() {
    let mut b = Box::new(Build::new());
    let f = b.add_filter(0, c(b"F\0"), c(b"F\0"));
    let m = b.add_machine(0, c(b"M\0"), c(b"M\0"));
    assert_eq!(b.filter_add_page(m), placeholder());
    assert_eq!(b.err, E_FORMAT);

    let mut b = Box::new(Build::new());
    let f2 = b.add_filter(0, c(b"F\0"), c(b"F\0"));
    assert_eq!(b.machine_add_page(f2, c(b"P\0"), c(b"P\0")), placeholder());
    assert_eq!(b.err, E_FORMAT);
    let _ = f;
}

#[test]
fn a_filter_page_lowers_a_draw_only_callback_to_a_module_relative_offset() {
    let mut b = gain();
    let page = &mut b.pages[0] as *mut CPage as *mut u8;
    b.page_canvas(page, 0, 0x4020, 0, 0);
    let (v2, n) = lowered(&b, 0x4000);
    let (v1, _) = lowered(&gain(), 0x4000);
    assert_eq!(n, 1);
    assert_eq!((rd(&v2, 0), rd(&v2, 4)), (KIND_FILTER_V2, LEN_FILTER_V2));
    assert_eq!(&v2[8..v1.len()], &v1[8..]);
    assert_eq!(&v2[v1.len()..], &0x20u32.to_be_bytes());
    assert_eq!(counts_of(&b, &DT2), (0, 1, 0));
}

#[test]
fn a_filter_graph_refuses_flags_an_event_an_argument_or_a_missing_draw() {
    for (flags, draw, event, arg) in [(1, 2, 0, 0), (0, 2, 4, 0), (0, 2, 0, 6), (0, 0, 0, 0)] {
        let mut b = gain();
        let page = &mut b.pages[0] as *mut CPage as *mut u8;
        b.page_canvas(page, flags, draw, event, arg);
        assert_eq!(b.err, E_FORMAT);
    }
}

#[test]
fn a_filter_page_has_no_bind_or_click() {
    let mut b = gain();
    let page = &mut b.pages[0] as *mut CPage as *mut u8;
    let prm = prm_of(&mut b);
    b.page_bind(page, prm, 0);
    assert_eq!(b.err, E_FORMAT);
    let mut b = gain();
    let prm = prm_of(&mut b);
    b.param_on_click(prm, 4, 5);
    assert_eq!(b.err, E_FORMAT);
}

fn prm_of(b: &mut Build) -> *mut u8 {
    &mut b.params[0] as *mut CParam as *mut u8
}

#[test]
fn a_range_that_does_not_fit_the_glyph_is_refused_at_lowering() {
    let mut b = gain();
    let p = prm_of(&mut b);
    b.param_glyph(p, 3); // on/off: 0 to 1
    b.param_range(p, 0, 5);
    let mut sink = RecordSink::measure();
    let k = dkrt_abi::keys::KeysCopy::empty();
    assert_eq!(
        lower(&b, &DT2, &k, &[true; MAX_M], 0, &mut sink, &mut Plan::new()),
        Err(E_FORMAT)
    );
}

#[test]
fn a_filter_module_is_not_empty_and_commits_with_a_parameter_row_per_knob() {
    let b = gain();
    assert!(!b.is_empty());
    let mut free = Free {
        machine_rows: 0,
        param_rows: 0,
        page_rows: 0,
    };
    let mut k = dkrt_abi::keys::KeysCopy::empty();
    assert_eq!(
        check_commit(&b, &DT2, &k, &free),
        E_DECL_USES,
        "the module did not declare the row in [uses]"
    );
    k.param_rows = 1;
    assert_eq!(
        check_commit(&b, &DT2, &k, &free),
        E_PARAM_FULL,
        "no free parameter row"
    );
    free.param_rows = 1;
    assert_eq!(check_commit(&b, &DT2, &k, &free), OK);
}

fn problems(bodies: &[Vec<u8>]) -> Vec<Problem> {
    let refs: Vec<&[u8]> = bodies.iter().map(|b| b.as_slice()).collect();
    let mut out = Vec::new();
    Machines::validate(KIND_FILTER, &refs, &mut |p| out.push(p));
    out
}

fn body_of_gain() -> Vec<u8> {
    lowered(&gain(), 0).0[8..].to_vec()
}

#[test]
fn the_record_rule_refuses_a_bad_size_key_knob_count_text_and_a_repeated_key() {
    let ok = body_of_gain();
    assert!(problems(&[ok.clone()]).is_empty());
    assert_eq!(
        problems(&[ok[..ok.len() - 1].to_vec()]).len(),
        1,
        "one byte short"
    );
    let mut bad = ok.clone();
    bad[..4].copy_from_slice(&0x0000_0005u32.to_be_bytes()); // a pinned number is no filter key
    assert_eq!(problems(&[bad]).len(), 1);
    let mut bad = ok.clone();
    bad[4..8].copy_from_slice(&3u32.to_be_bytes()); // three own knobs
    assert_eq!(problems(&[bad]).len(), 1);
    let mut bad = ok.clone();
    bad[8] = 0x07; // not printable
    assert_eq!(problems(&[bad]).len(), 1);
    let mut bad = ok.clone();
    bad[56 + 4..56 + 8].copy_from_slice(&(200u32 * 256).to_be_bytes()); // minimum above maximum
    assert_eq!(problems(&[bad]).len(), 1);
    let mut bad = ok.clone();
    bad[56 + 64] = 1; // a slot beyond the knob count holds data
    assert_eq!(problems(&[bad]).len(), 1);
    let twice = problems(&[ok.clone(), ok.clone()]);
    assert_eq!(twice.len(), 1);
    assert_eq!(
        twice[0].with,
        Some(0),
        "the second record clashes with the first"
    );
    let nine: Vec<Vec<u8>> = (0..9)
        .map(|i| {
            let mut b = ok.clone();
            b[..4].copy_from_slice(&(KEY + 0x100 * (i + 1)).to_be_bytes());
            b
        })
        .collect();
    assert_eq!(problems(&nine).len(), 1, "a module has at most 8 filters");
}

/// A section of the lowered records of `b` with the key set to `key` (16-byte header, count at 12).
fn section(key: u32) -> Vec<u8> {
    let mut b = gain();
    b.filters[0].key = key;
    let (recs, n) = lowered(&b, 0);
    let mut s = std::vec![0u8; 16];
    s[12..16].copy_from_slice(&n.to_be_bytes());
    s.extend_from_slice(&recs);
    s
}

fn core_with(modules: &[&[u32]]) -> (FakeCore, SettleCtx) {
    let mut core = FakeCore::new();
    core.device = dkrt_abi::profiles::DT2.id;
    let mut sel = SettleCtx::new();
    sel.count = modules.len();
    for (i, keys) in modules.iter().enumerate() {
        core.set_keys(i as u32, KIND_FILTER, keys);
    }
    (core, sel)
}

#[test]
fn the_numbers_are_16_to_31_sticky_in_their_own_map_and_the_17th_filter_gets_none() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let k = |n: u32| 0x0010_0000 + 0x100 * n;
    let eight: Vec<u32> = (0..8).map(k).collect();
    let more: Vec<u32> = (8..16).map(k).collect();
    let last: Vec<u32> = std::vec![k(16)];
    let (core, sel) = core_with(&[&eight, &more, &last]);
    unsafe { <Machines as Service<FakeCore>>::settle(&core, &sel) };
    // without a map the numbers are the lowest free ones, in load order; 17 filters want 16 numbers
    core.boot_index.set(0);
    unsafe {
        assert_eq!(filter::resolve(&core, k(0)), 16);
        assert_eq!(filter::resolve(&core, k(7)), 23);
    }
    core.boot_index.set(1);
    unsafe { assert_eq!(filter::resolve(&core, k(15)), 31) };
    core.boot_index.set(2);
    unsafe { assert_eq!(filter::resolve(&core, k(16)), 0, "the range is used up") };
    // the filter's check refuses it and passes the one that has a number
    let reg = section(k(16));
    assert_eq!(
        unsafe { filter::check(&core, reg.as_ptr(), 0) },
        E_TYPE_USED
    );
    core.boot_index.set(1);
    let reg = section(k(9));
    assert_eq!(unsafe { filter::check(&core, reg.as_ptr(), 0) }, OK);
    // the map is saved under its own key (16 words), not the machines' key
    let saved = core
        .settings
        .borrow()
        .iter()
        .find(|(key, _)| key == SETTING)
        .map(|(_, v)| v.clone())
        .expect("the map is stored");
    assert_eq!(saved.len(), 64);
    assert!(!core
        .settings
        .borrow()
        .iter()
        .any(|(key, _)| key == b"machines.types"));
    assert_eq!(rd(&saved, 0), k(0) | 16);
}

#[test]
fn a_filter_keeps_its_number_when_an_earlier_filter_leaves() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (a, b) = (0x00a1_0000u32, 0x00b2_0000u32);
    let (core, sel) = core_with(&[&[a, b]]);
    unsafe { <Machines as Service<FakeCore>>::settle(&core, &sel) };
    core.boot_index.set(0);
    unsafe {
        assert_eq!(
            (filter::resolve(&core, a), filter::resolve(&core, b)),
            (16, 17)
        );
    }
    // the next boot has only b: the stored map keeps its 17
    let (core2, sel2) = core_with(&[&[b]]);
    *core2.settings.borrow_mut() = core.settings.borrow().clone();
    unsafe { <Machines as Service<FakeCore>>::settle(&core2, &sel2) };
    core2.boot_index.set(0);
    unsafe { assert_eq!(filter::resolve(&core2, b), 17) };
}

#[test]
fn live_filter_numbering_preserves_active_rows_and_saves_only_on_commit() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let extra = KEY + 0x100;
    let (core, sel) = core_with(&[&[KEY]]);
    core.boot_index.set(0);
    let settle = |sel: &SettleCtx| unsafe { <Machines as Service<FakeCore>>::settle(&core, sel) };
    settle(&sel);
    let reg = section(KEY);
    unsafe { filter::apply(&core, 1, reg.as_ptr(), 0) };
    let writes = core.calls().len();
    core.set_keys(1, KIND_FILTER, &[extra]);
    core.boot_index.set(1);

    settle(&SettleCtx::preview(2));
    assert_eq!(unsafe { filter::resolve(&core, extra) }, 17);
    assert_eq!(filter::rows()[0].owner, 1);
    assert_eq!(core.calls().len(), writes);

    settle(&SettleCtx::restore(1));
    assert_eq!(unsafe { filter::resolve(&core, extra) }, 0);
    assert_eq!(filter::rows()[0].owner, 1);
    assert_eq!(core.calls().len(), writes);

    settle(&SettleCtx::preview(2));
    settle(&SettleCtx::commit(2));
    assert_eq!(unsafe { filter::resolve(&core, extra) }, 17);
    let settings = core.settings.borrow();
    let (_, map) = settings.iter().find(|(k, _)| k == SETTING).unwrap();
    assert_eq!(rd(map, 4), extra | 17);
    filter::remove(1);
}

#[test]
fn filter_graph_offsets_must_be_aligned_and_within_the_coldfire_image() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let (core, sel) = core_with(&[&[KEY]]);
    unsafe { <Machines as Service<FakeCore>>::settle(&core, &sel) };
    core.boot_index.set(0);
    let mut b = gain();
    let page = &mut b.pages[0] as *mut CPage as *mut u8;
    b.page_canvas(page, 0, 0x4020, 0, 0);
    let (recs, n) = lowered(&b, 0x4000);
    let mut reg = vec![0u8; RecordSink::HEADER];
    reg[12..16].copy_from_slice(&n.to_be_bytes());
    reg.extend_from_slice(&recs);

    assert_eq!(unsafe { filter::check(&core, reg.as_ptr(), 0x100) }, OK);
    assert_eq!(
        unsafe { filter::check(&core, reg.as_ptr(), 0x20) },
        E_FORMAT
    );
    *reg.last_mut().unwrap() = 0x21;
    assert_eq!(
        unsafe { filter::check(&core, reg.as_ptr(), 0x100) },
        E_FORMAT
    );
    *reg.last_mut().unwrap() = 0x20;

    reg[12..16].copy_from_slice(&2u32.to_be_bytes());
    reg.extend_from_slice(&lowered(&gain(), 0x4000).0);
    assert_eq!(
        unsafe { filter::check(&core, reg.as_ptr(), 0x100) },
        E_FORMAT
    );
}
