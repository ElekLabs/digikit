//! Host tests of the `lfo` service over `FakeCore`: what it declares, how it numbers, checks, applies and removes `lfo_shape` records, how the interrupt
//! site's lookup reads the table, and that it ignores a record of another service. The statics of the crate (the shape table, the settled keys) are shared,
//! so each test takes `LOCK` and starts from an empty table.
//! The runtime's own tests (`handlers/lfo_tests.rs` in the runtime crate) drive the same service through the collector, the registry walkers and the planner.
use dkrt_abi::mem::FwMem;
use dkrt_abi::provider::{Consumer, ConsumerId, Provider, Records};
use dkrt_abi::service::{check_step, CheckCtx, SettleCtx};
use dkrt_sdk::records::{KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_sdk::*;
use dkrt_svc_lfo::graph;
use dkrt_svc_lfo::lfo::Lfo;
use dkrt_svc_lfo::records::{MAX_SHAPES, NAME_LEN, RANGE, SAMPLES, SETTING};
use dkrt_svc_lfo::shapes::{
    eval, highest, interp, name_into, name_of, owner_of, shape, tri, unipolar_of, used,
};
use dkrt_svc_lfo::wave::{set_firmware, wave_max};
use dkrt_test_support::FakeCore;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;

static LOCK: Mutex<()> = Mutex::new(());

/// The lock, with an empty shape table and no settled keys, and a started owner object.
fn fresh() -> (MutexGuard<'static, ()>, FakeCore, Lfo) {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = FakeCore::new();
    set_firmware(0, [0; 3]);
    let mut lfo = *dkrt_test_support::start_boxed::<Lfo>(&core).unwrap();
    for owner in 0..=8u8 {
        detach(&mut lfo, &core, owner);
    }
    Provider::settle(&mut lfo, &core, &SettleCtx::new());
    assert_eq!(used(), 0);
    (g, core, lfo)
}

/// A module goes down: `Provider::detach`.
fn detach(lfo: &mut Lfo, core: &FakeCore, owner: u8) {
    Provider::detach(lfo, core, ConsumerId::new(owner));
}

/// `settle` over modules that provide these shape keys (module `k` at selection index `k`).
fn settle(lfo: &mut Lfo, core: &FakeCore, modules: &[&[u32]]) {
    let mut sel = SettleCtx::new();
    sel.count = modules.len();
    for (k, keys) in modules.iter().enumerate() {
        core.set_keys(k as u32, KIND_LFO_SHAPE, keys);
    }
    Provider::settle(lfo, core, &sel);
}

/// The body of a shape record: the key, the name, then the 32 samples (a ramp from `from`), big-endian.
fn body(key: u32, name: &[u8; NAME_LEN], from: i16) -> Vec<u8> {
    let mut b = key.to_be_bytes().to_vec();
    b.extend_from_slice(name);
    for s in 0..SAMPLES as i16 {
        b.extend_from_slice(&(from + 100 * s).to_be_bytes());
    }
    b
}

/// A `register` section of shape records: the 16-byte header (the count at word 3), then each record's kind, length and body.
fn section(shapes: &[(u32, i16)]) -> Vec<u8> {
    let mut s = vec![0u8; 16];
    s[12..16].copy_from_slice(&(shapes.len() as u32).to_be_bytes());
    for (i, (key, from)) in shapes.iter().enumerate() {
        s.extend_from_slice(&KIND_LFO_SHAPE.to_be_bytes());
        s.extend_from_slice(&LEN_LFO_SHAPE.to_be_bytes());
        s.extend_from_slice(&body(*key, &[b'T', b'S', b'A' + i as u8, b'T'], *from));
    }
    s
}

fn check(lfo: &mut Lfo, core: &FakeCore, step: u32, sec: &[u8]) -> i32 {
    let c = CheckCtx {
        reg: sec.as_ptr(),
        image_size: 0x1000,
    };
    Records::check(lfo, core, step, &c).map_or_else(|e| e, |()| OK)
}

fn apply(lfo: &mut Lfo, core: &FakeCore, owner: u8, kind: u32, body: &[u8]) -> i32 {
    let records = [(kind, body)];
    Records::apply(
        lfo,
        core,
        kind,
        body,
        &Consumer::new(ConsumerId::new(owner), &records),
    )
    .map_or_else(|e| e, |()| OK)
}

const KEY_A: u32 = 0x00a1_0100;
const KEY_B: u32 = 0x00b2_0200;

#[test]
fn a_record_is_the_header_the_key_the_name_and_the_samples() {
    assert_eq!(LEN_LFO_SHAPE, 8 + 4 + NAME_LEN as u32 + 2 * SAMPLES as u32);
}

#[test]
fn the_wave_numbers_are_above_rand_and_below_the_stock_sign_bit() {
    assert_eq!(RANGE.lo, 7);
    assert_eq!(RANGE.hi - RANGE.lo + 1, MAX_SHAPES as u32);
    assert!(RANGE.hi < 0x80);
}

#[test]
fn a_shape_gets_a_number_goes_through_check_apply_and_remove() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    assert_eq!(
        lfo.number_of(0, KEY_A),
        7,
        "the first key takes the first number"
    );
    core.boot_index.set(0);
    let sec = section(&[(KEY_A, -1000)]);
    assert_eq!(check(&mut lfo, &core, check_step::RECORDS, &sec), OK);
    assert_eq!(check(&mut lfo, &core, check_step::ROWS, &sec), OK);
    assert_eq!(
        apply(
            &mut lfo,
            &core,
            1,
            KIND_LFO_SHAPE,
            &body(KEY_A, b"RAMP", -1000)
        ),
        OK
    );
    let s = shape(7).expect("Records::apply kept the shape");
    assert_eq!((s[0], s[1], s[31]), (-1000, -900, 2100));
    assert_eq!((owner_of(7), used()), (1, 1));
    assert_eq!(name_of(7), Some(*b"RAMP"));
    assert!(!unipolar_of(7), "a shape with a negative sample is bipolar");
    assert!(
        graph::tiles_of(7).is_some(),
        "the page's tiles are published with the shape"
    );
    detach(&mut lfo, &core, 2);
    assert_eq!(used(), 1, "another module's removal keeps the shape");
    assert!(graph::tiles_of(7).is_some());
    detach(&mut lfo, &core, 1);
    assert_eq!((shape(7), owner_of(7), used()), (None, 0, 0));
    assert!(
        graph::tiles_of(7).is_none(),
        "the tiles are withdrawn with the shape"
    );
}

#[test]
fn the_numbers_are_kept_in_the_settings_key_across_boots() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!((lfo.number_of(0, KEY_A), lfo.number_of(1, KEY_B)), (7, 8));
    let mut raw = [0u8; MAX_SHAPES * 4];
    assert_eq!(
        unsafe { dkrt_abi::core_api::CoreApi::settings_get(&core, SETTING, &mut raw) },
        raw.len() as i32
    );
    assert_eq!(
        u32::from_be_bytes([raw[0], raw[1], raw[2], raw[3]]),
        KEY_A | 7
    );
    // the next boot loads only B: it keeps number 8
    settle(&mut lfo, &core, &[&[KEY_B]]);
    assert_eq!(lfo.number_of(0, KEY_B), 8);
}

#[test]
fn a_key_that_the_module_does_not_provide_or_a_bad_name_or_a_key_twice_is_refused() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A, 0x00c3_0300]]);
    core.boot_index.set(0);
    assert_eq!(
        check(&mut lfo, &core, check_step::RECORDS, &section(&[(0, 0)])),
        E_FORMAT
    );
    assert_eq!(
        check(
            &mut lfo,
            &core,
            check_step::RECORDS,
            &section(&[(0x0099_9900, 0)])
        ),
        E_FORMAT,
        "a literal key that the KEYS entry does not list"
    );
    assert_eq!(
        check(
            &mut lfo,
            &core,
            check_step::RECORDS,
            &section(&[(KEY_A, 0), (KEY_A, 1)])
        ),
        E_TYPE_USED
    );
    assert_eq!(
        check(
            &mut lfo,
            &core,
            check_step::RECORDS,
            &section(&[(KEY_A, 0), (0x00c3_0300, 1)])
        ),
        OK
    );
    let mut sec = section(&[(KEY_A, 0)]);
    sec[16 + 8 + 4] = 0x07; // a control character in the name
    assert_eq!(check(&mut lfo, &core, check_step::RECORDS, &sec), E_FORMAT);
}

#[test]
fn a_key_without_a_number_and_a_full_table_are_refused() {
    let (_g, core, mut lfo) = fresh();
    let keys: Vec<u32> = (0..MAX_SHAPES as u32)
        .map(|i| 0x0010_0000 + (i << 8))
        .collect();
    settle(&mut lfo, &core, &[&keys[..4], &keys[4..]]);
    core.boot_index.set(0);
    assert_eq!(
        apply(
            &mut lfo,
            &core,
            1,
            KIND_LFO_SHAPE,
            &body(keys[0], b"A   ", 0)
        ),
        OK
    );
    // a key that settle gave no number (a ninth key of the boot) has no place
    let nine: Vec<u32> = (0..MAX_SHAPES as u32 + 1)
        .map(|i| 0x0020_0000 + (i << 8))
        .collect();
    settle(&mut lfo, &core, &[&nine[..5], &nine[5..]]);
    core.boot_index.set(1);
    assert_eq!(lfo.number_of(1, nine[MAX_SHAPES]), 0);
    assert_eq!(
        check(
            &mut lfo,
            &core,
            check_step::ROWS,
            &section(&[(nine[MAX_SHAPES], 0)])
        ),
        E_TABLE_FULL
    );
    assert_eq!(
        apply(
            &mut lfo,
            &core,
            1,
            KIND_LFO_SHAPE,
            &body(nine[MAX_SHAPES], b"X   ", 0)
        ),
        E_TABLE_FULL
    );
}

/// Defence in depth: a slot held by another module is not overwritten, while the module being reloaded may still hold its own.
#[test]
fn a_slot_of_another_module_is_not_overwritten() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"A   ", 0)),
        OK
    );
    assert_eq!(
        apply(&mut lfo, &core, 2, KIND_LFO_SHAPE, &body(KEY_A, b"B   ", 0)),
        E_TYPE_USED
    );
    assert_eq!(
        owner_of(lfo.number_of(0, KEY_A)),
        1,
        "the holder is unchanged"
    );
    core.live_op.set(1);
    assert_eq!(
        apply(&mut lfo, &core, 2, KIND_LFO_SHAPE, &body(KEY_A, b"B   ", 0)),
        OK,
        "the module being reloaded"
    );
    core.live_op.set(0);
}

/// A reload of a module that keeps shapes: its own shapes are still in the table at the check of its new file (the teardown that precedes the load gives them back),
/// so they do not count against the table; another module's shapes do. No other number check exists: `settle` gives each key its own number.
#[test]
fn the_shapes_of_the_module_being_reloaded_do_not_count_against_the_table() {
    let (_g, core, mut lfo) = fresh();
    let keys: Vec<u32> = (0..MAX_SHAPES as u32)
        .map(|i| 0x0010_0000 + (i << 8))
        .collect();
    settle(&mut lfo, &core, &[&keys[..5], &keys[5..]]);
    core.boot_index.set(0);
    for (i, k) in keys[..5].iter().enumerate() {
        assert_eq!(
            apply(
                &mut lfo,
                &core,
                1,
                KIND_LFO_SHAPE,
                &body(*k, b"A   ", i as i16)
            ),
            OK
        );
    }
    core.boot_index.set(1); // the second module's shape has its number under its own selection index
    assert_eq!(
        apply(
            &mut lfo,
            &core,
            2,
            KIND_LFO_SHAPE,
            &body(keys[5], b"B   ", 0)
        ),
        OK
    );
    core.boot_index.set(0);
    // module 1 (owner 1) is replaced by a file of the same five shapes: the table holds six, one more would make nine
    let same: Vec<(u32, i16)> = keys[..5].iter().map(|k| (*k, 0)).collect();
    let mut run = |op: u32| {
        core.live_op.set(op);
        let r = check(&mut lfo, &core, check_step::ROWS, &section(&same));
        core.live_op.set(0);
        r
    };
    assert_eq!(run(1), OK, "the reload of the owner");
    assert_eq!(
        run(0),
        E_TABLE_FULL,
        "a load of the same five (as another module) does not fit: 6 + 5 > 8"
    );
    assert_eq!(
        run(2),
        E_TABLE_FULL,
        "the reload of the module that keeps one shape frees one"
    );
}

#[test]
fn a_record_of_another_service_and_the_other_check_steps_are_not_its_business() {
    let (_g, core, mut lfo) = fresh();
    assert_eq!(apply(&mut lfo, &core, 1, 999, &body(KEY_A, b"TEST", 0)), OK);
    assert_eq!(used(), 0);
    let dup = section(&[(9, 0), (9, 1)]);
    assert_eq!(check(&mut lfo, &core, check_step::COUNTS, &dup), OK);
    assert_eq!(check(&mut lfo, &core, check_step::NATIVE, &dup), OK);
}

#[test]
fn each_write_to_the_table_takes_the_interrupt_mask_once_and_releases_it() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    let before = core.host.masks_taken.get();
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"RAMP", 0)),
        OK
    );
    assert_eq!(
        core.host.masks_taken.get(),
        before + 1,
        "the shape is stored under the mask"
    );
    assert_eq!(core.host.masked.get(), 0);
    detach(&mut lfo, &core, 1);
    assert_eq!(
        core.host.masks_taken.get(),
        before + 2,
        "the slot is freed under the mask"
    );
    assert_eq!(core.host.masked.get(), 0);
}

#[test]
fn a_body_shorter_than_a_shape_record_is_refused_as_a_format_error() {
    // `check` fixes the length first, so this is defence in depth: no real boot reaches it
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    let short = body(KEY_A, b"RAMP", 0);
    assert_eq!(
        apply(
            &mut lfo,
            &core,
            1,
            KIND_LFO_SHAPE,
            &short[..short.len() - 1]
        ),
        E_FORMAT
    );
    assert_eq!(used(), 0);
}

/// `name_into` for the WAVE value `value` with a buffer of `width + 1` bytes filled with 0xaa: (characters written, the buffer).
fn name(value: i32, width: usize) -> (usize, [u8; 8]) {
    let mut buf = [0xaau8; 8];
    let n = unsafe { name_into(value, buf.as_mut_ptr(), width) };
    (n, buf)
}

/// Loads shape `key` at selection index `at` as record owner `owner` with `name`.
fn load(lfo: &mut Lfo, core: &FakeCore, owner: u8, key: u32, nm: &[u8; NAME_LEN]) {
    core.boot_index.set(0);
    assert_eq!(
        apply(lfo, core, owner, KIND_LFO_SHAPE, &body(key, nm, 0)),
        OK
    );
}

#[test]
fn the_name_formatters_get_the_name_of_a_loaded_shape_and_stock_behaviour_otherwise() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A, KEY_B]]);
    // nothing loaded: no name for any wave number, in or out of the range
    for wave in [0i32, 6, 7, 8, 14, 15, 127] {
        assert_eq!(name(wave << 8, 4).0, 0, "wave {wave}");
    }
    load(&mut lfo, &core, 1, KEY_A, b"RAMP");
    let (n, buf) = name(7 << 8, 4);
    assert_eq!(
        (n, &buf[..6]),
        (4, &b"RAMP\0\xaa"[..]),
        "four characters and the NUL"
    );
    // the low byte of the value is ignored, as the stock formatter does
    assert_eq!(name((7 << 8) | 0x7f, 4).0, 4);
    // the three-character formatter of the Digitone II: the first three characters, then the NUL
    let (n, buf) = name(7 << 8, 3);
    assert_eq!((n, &buf[..5]), (3, &b"RAM\0\xaa"[..]));
    // a number that no module keeps (8 is assigned, not loaded), stock numbers, numbers above the range and negative values: no name, nothing written
    for value in [8 << 8, 6 << 8, 15 << 8, 127 << 8, -1, i32::MIN, 0] {
        let (n, buf) = name(value, 4);
        assert_eq!((n, buf), (0, [0xaa; 8]), "value {value:#x}");
    }
    load(&mut lfo, &core, 2, KEY_B, b"SAW2");
    assert_eq!(name(8 << 8, 4).0, 4);
    // wave 7 is now a gap below the highest loaded number (8): it prints dashes, four or three, and the tick plays TRI for it
    detach(&mut lfo, &core, 1);
    let (n, buf) = name(7 << 8, 4);
    assert_eq!((n, &buf[..6]), (4, &b"----\0\xaa"[..]), "a gap: dashes");
    let (n, buf) = name(7 << 8, 3);
    assert_eq!(
        (n, &buf[..5]),
        (3, &b"---\0\xaa"[..]),
        "a gap on the short formatter"
    );
    // above the highest loaded number and below the range: stock "ERR" (no name)
    assert_eq!(name(9 << 8, 4).0, 0);
    assert_eq!(name(6 << 8, 4).0, 0);
    detach(&mut lfo, &core, 2);
    assert_eq!(
        name(7 << 8, 4).0,
        0,
        "no shape loaded: stock formatter again"
    );
    assert_eq!(name(8 << 8, 4).0, 0);
}

#[test]
fn a_name_shorter_than_the_width_returns_its_length_up_to_the_first_nul() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A, KEY_B]]);
    load(&mut lfo, &core, 1, KEY_A, b"AB\0\0");
    let (n, buf) = name(7 << 8, 4);
    assert_eq!(
        (n, &buf[..4]),
        (2, &b"AB\0\xaa"[..]),
        "two characters, then the NUL"
    );
    // a NUL inside the width ends the name on the short formatter too, and the width still caps a longer name
    load(&mut lfo, &core, 2, KEY_B, b"X\0YZ");
    let (n, buf) = name(8 << 8, 3);
    assert_eq!((n, &buf[..3]), (1, &b"X\0\xaa"[..]));
    let (n, buf) = name(7 << 8, 1);
    assert_eq!((n, &buf[..3]), (1, &b"A\0\xaa"[..]), "width 1");
}

#[test]
fn the_wave_maximum_follows_the_highest_loaded_wave_number() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!(
        (highest(), wave_max()),
        (0, 0),
        "no shape: 0 leaves the stock maximum 0x600"
    );
    // the second key is loaded first: the maximum is its number, not the count
    core.boot_index.set(1);
    assert_eq!(
        apply(&mut lfo, &core, 2, KIND_LFO_SHAPE, &body(KEY_B, b"UP  ", 0)),
        OK
    );
    assert_eq!((highest(), wave_max()), (8, 0x800));
    core.boot_index.set(0);
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"DOWN", 0)),
        OK
    );
    assert_eq!((highest(), wave_max()), (8, 0x800));
    detach(&mut lfo, &core, 2);
    assert_eq!((highest(), wave_max()), (7, 0x700));
    detach(&mut lfo, &core, 1);
    assert_eq!((highest(), wave_max()), (0, 0));
}

/// The address of the stock table and of its RAM copy in the fake image, and the registry's first row (the WAVE codes are stock rows below it).
const STOCK: u32 = 0x1000;
const COPY: u32 = 0x8000;
const CODES: [u32; 3] = [79, 89, 99];

/// An image with the registry's parameter table (`P_TABLE` = the copy, 60-byte rows, the registry from row 300) and the three WAVE rows in the copy (0..0x600, defaults 0x100, 0, 0);
/// the service knows the stock addresses (`set_firmware`, as `Lfo::start_in` gives them from the bound anchors).
fn image_with_rows(core: &mut FakeCore) {
    let mut t = vec![0u32; 90];
    t[20] = COPY; // P_TABLE
    t[27] = 60; // PF_SIZE
    t[23] = 300; // P_FIRST
    core.set_rows_info(t);
    for (c, def) in CODES.iter().zip([0x100, 0, 0]) {
        unsafe {
            core.wr(COPY + 60 * c + 8, 0);
            core.wr(COPY + 60 * c + 12, 0x600);
            core.wr(COPY + 60 * c + 16, def);
        }
    }
    set_firmware(STOCK, CODES.map(|c| STOCK + 60 * c));
}

/// The maximum words of the three WAVE rows in the copy.
fn wave_words(core: &FakeCore) -> [u32; 3] {
    CODES.map(|c| unsafe { core.rd(COPY + 60 * c + 12) })
}

#[test]
fn the_wave_rows_of_the_copy_follow_the_highest_loaded_shape_and_go_back_to_stock() {
    let (_g, mut core, mut lfo) = fresh();
    image_with_rows(&mut core);
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!(wave_words(&core), [0x600; 3], "no shape: the stock maximum");
    core.boot_index.set(1);
    assert_eq!(
        apply(&mut lfo, &core, 2, KIND_LFO_SHAPE, &body(KEY_B, b"UP  ", 0)),
        OK
    );
    assert_eq!(wave_words(&core), [0x800; 3], "the second key has number 8");
    core.boot_index.set(0);
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"DOWN", 0)),
        OK
    );
    assert_eq!(wave_words(&core), [0x800; 3]);
    detach(&mut lfo, &core, 2);
    assert_eq!(
        wave_words(&core),
        [0x700; 3],
        "the highest left is number 7"
    );
    // a reload: the same module goes and comes back
    detach(&mut lfo, &core, 1);
    assert_eq!(
        wave_words(&core),
        [0x600; 3],
        "the last shape is gone: the word is back at its start value"
    );
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"DOWN", 0)),
        OK
    );
    assert_eq!(wave_words(&core), [0x700; 3]);
    detach(&mut lfo, &core, 1);
    assert_eq!(wave_words(&core), [0x600; 3]);
    // the minimum and the default of the rows and the stock addresses were never touched
    for (c, def) in CODES.iter().zip([0x100, 0, 0]) {
        unsafe {
            assert_eq!(
                (core.rd(COPY + 60 * c + 8), core.rd(COPY + 60 * c + 16)),
                (0, def)
            );
            assert_eq!(
                core.rd(STOCK + 60 * c + 12),
                0,
                "the stock table is not written"
            );
        }
    }
    assert!(
        core.mem
            .borrow()
            .keys()
            .all(|a| (COPY..COPY + 60 * 300).contains(a)),
        "only the copy was written"
    );
    set_firmware(0, [0; 3]);
}

/// A service reload: the consumers detach first (the shape modules go down), the service stops, a new owner object starts over the same core, and the consumers come back,
/// in another order. The wave numbers come from the sticky setting, so a sound that names one keeps its shape, and the rows of the parameter table hold the stock maximum
/// while no shape is loaded, which is where the new owner reads its stock pair from.
#[test]
fn a_service_started_again_numbers_the_same_keys_with_the_same_waves_and_finds_the_rows_at_stock() {
    let (_g, mut core, mut lfo) = fresh();
    image_with_rows(&mut core);
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    core.boot_index.set(0);
    assert_eq!(apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"DOWN", 0)), OK);
    core.boot_index.set(1);
    assert_eq!(apply(&mut lfo, &core, 2, KIND_LFO_SHAPE, &body(KEY_B, b"UP  ", 0)), OK);
    let (a, b) = (shape(7).unwrap(), shape(8).unwrap());
    assert_eq!(wave_words(&core), [0x800; 3]);
    // the closure goes down, consumers first: nothing of the shapes or the raised maximum is left when the service stops
    detach(&mut lfo, &core, 2);
    detach(&mut lfo, &core, 1);
    assert_eq!((used(), highest(), wave_max()), (0, 0, 0));
    assert_eq!(wave_words(&core), [0x600; 3], "the stock maximum is back before the old service stops");
    drop(lfo);
    // the new owner object (`Provider::start_in` makes it from zero bytes) and the consumers back in the other order
    image_with_rows(&mut core);
    let mut lfo = *dkrt_test_support::start_boxed::<Lfo>(&core).unwrap();
    assert_eq!((lfo.number_of(0, KEY_A), lfo.number_of(0, KEY_B)), (0, 0), "no number before the settle");
    settle(&mut lfo, &core, &[&[KEY_B], &[KEY_A]]);
    assert_eq!((lfo.number_of(1, KEY_A), lfo.number_of(0, KEY_B)), (7, 8), "the numbers are the sticky ones, not the order of the selection");
    core.boot_index.set(1);
    assert_eq!(apply(&mut lfo, &core, 2, KIND_LFO_SHAPE, &body(KEY_A, b"DOWN", 0)), OK);
    core.boot_index.set(0);
    assert_eq!(apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_B, b"UP  ", 0)), OK);
    assert_eq!((shape(7), shape(8)), (Some(a), Some(b)), "the same waves hold the same shapes");
    assert_eq!(wave_words(&core), [0x800; 3]);
    detach(&mut lfo, &core, 1);
    detach(&mut lfo, &core, 2);
    assert_eq!(wave_words(&core), [0x600; 3], "the stock pair the new owner read goes back, not a raised one");
    set_firmware(0, [0; 3]);
}

#[test]
fn an_image_without_the_registry_table_refuses_a_shape_and_writes_nothing() {
    let (_g, core, mut lfo) = fresh();
    // the service has the firmware addresses, the image has no rows_info
    set_firmware(STOCK, CODES.map(|c| STOCK + 60 * c));
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    let sec = section(&[(KEY_A, 0)]);
    assert_eq!(
        check(&mut lfo, &core, check_step::ROWS, &sec),
        E_NO_REGISTRY
    );
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"RAMP", 0)),
        E_NO_REGISTRY
    );
    detach(&mut lfo, &core, 1);
    assert_eq!(used(), 0, "`Provider::detach` takes the shape back");
    assert!(core.mem.borrow().is_empty(), "no word was written");
    // a module with no shape has nothing to refuse
    let none = section(&[]);
    assert_eq!(check(&mut lfo, &core, check_step::ROWS, &none), OK);
    set_firmware(0, [0; 3]);
}

#[test]
fn a_build_without_firmware_rows_loads_shapes_and_writes_no_row() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    let sec = section(&[(KEY_A, 0)]);
    assert_eq!(check(&mut lfo, &core, check_step::ROWS, &sec), OK);
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"RAMP", 0)),
        OK
    );
    assert!(core.mem.borrow().is_empty());
}

/// The 32 samples `s[i] = 1000 * i`, so that every value of the interpolation is easy to compute by hand.
fn ramp() -> [i16; SAMPLES] {
    let mut s = [0i16; SAMPLES];
    for (i, v) in s.iter_mut().enumerate() {
        *v = 1000 * i as i16;
    }
    s
}

#[test]
fn the_interpolation_gives_the_sample_at_a_cell_start_and_the_mean_at_its_middle() {
    let s = ramp();
    // phase 0: sample 0, as Q31 (Q15 << 16)
    assert_eq!(interp(&s, 0), 0);
    // cell 3 starts at 3 << 27: sample 3000
    assert_eq!(interp(&s, 3 << 27), 3000 << 16);
    // the middle of cell 3: fraction 0x4000 of 0x8000 -> (3000 + 4000) / 2 = 3500
    assert_eq!(interp(&s, (3 << 27) | (1 << 26)), 3500 << 16);
    // a quarter: 3250
    assert_eq!(interp(&s, (3 << 27) | (1 << 25)), 3250 << 16);
}

#[test]
fn the_last_cell_wraps_to_the_first_sample() {
    let s = ramp();
    // cell 31 is 31000, the next is sample 0: the middle is 15500
    assert_eq!(interp(&s, (31 << 27) | (1 << 26)), 15500 << 16);
    // a full swing across one cell keeps the 32-bit result exact: -32768 to 32767
    let mut t = [0i16; SAMPLES];
    t[0] = -32768;
    t[1] = 32767;
    assert_eq!(interp(&t, 0), -32768 << 16);
    assert_eq!(interp(&t, 1 << 26), (-32768 << 16) + (65535 << 15));
}

#[test]
fn the_fallback_is_the_stock_triangle() {
    // the stock function: 0 at phase 0, full scale at a quarter, -1 at a half, negative full scale at three quarters
    assert_eq!(tri(0), 0);
    assert_eq!(tri(0x4000_0000), 0x7fff_ffff);
    assert_eq!(tri(0x8000_0000u32), -1);
    assert_eq!(tri(0xc000_0000u32), i32::MIN);
}

#[test]
fn eval_reads_the_shape_of_the_number_and_falls_back_to_tri_when_no_module_keeps_it() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    assert_eq!(
        eval(7, 0x4000_0000),
        tri(0x4000_0000),
        "no module keeps number 7 yet"
    );
    assert_eq!(
        apply(
            &mut lfo,
            &core,
            1,
            KIND_LFO_SHAPE,
            &body(KEY_A, b"RAMP", -1000)
        ),
        OK
    );
    assert_eq!(eval(7, 0), -1000 << 16);
    assert_eq!(eval(7, 1 << 26), -950 << 16);
    assert_eq!(eval(8, 1 << 26), tri(1 << 26), "number 8 has no shape");
    assert_eq!(
        eval(100, 0x4000_0000),
        tri(0x4000_0000),
        "a number outside the range"
    );
    detach(&mut lfo, &core, 1);
    assert_eq!(eval(7, 0), tri(0), "an unloaded shape falls back");
}

#[test]
fn a_shape_without_a_negative_sample_is_unipolar() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    assert_eq!(
        apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"UNI ", 0)),
        OK
    );
    assert!(unipolar_of(7));
    assert!(!unipolar_of(8));
}

/// A fixed pseudo-random sequence (xorshift32), so a failure repeats.
fn next(x: &mut u32) -> u32 {
    *x ^= *x << 13;
    *x ^= *x >> 17;
    *x ^= *x << 5;
    *x
}

#[test]
fn the_delta_table_gives_the_same_value_as_the_plain_formula_for_random_tables_and_phases() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    let mut x = 0x2468_ace1u32;
    for round in 0..40 {
        let mut samples = [0i16; SAMPLES];
        for (i, v) in samples.iter_mut().enumerate() {
            *v = match round {
                // the largest steps: full swing up and down
                0 => {
                    if i % 2 == 0 {
                        i16::MIN
                    } else {
                        i16::MAX
                    }
                }
                1 => {
                    if i % 2 == 0 {
                        i16::MAX
                    } else {
                        i16::MIN
                    }
                }
                _ => next(&mut x) as i16,
            };
        }
        let mut b = KEY_A.to_be_bytes().to_vec();
        b.extend_from_slice(b"RND ");
        for v in samples {
            b.extend_from_slice(&v.to_be_bytes());
        }
        detach(&mut lfo, &core, 1);
        assert_eq!(apply(&mut lfo, &core, 1, KIND_LFO_SHAPE, &b), OK);
        assert_eq!(shape(7), Some(samples), "the samples are read back");
        let mut phases = vec![
            0,
            u32::MAX,
            0x7fff_ffff,
            0x8000_0000,
            31 << 27,
            (31 << 27) | 0x07ff_ffff,
        ];
        for _ in 0..2000 {
            phases.push(next(&mut x));
        }
        for p in phases {
            assert_eq!(
                eval(7, p),
                interp(&samples, p),
                "round {round} phase {p:#x}"
            );
        }
    }
}

/// A reload runs `settle` again over the live modules and the new file in the old slot: loaded shapes keep their numbers, a shape the new file adds takes a free one,
/// and a second run over the same set changes nothing.
#[test]
fn a_second_settle_over_the_live_set_keeps_every_number_and_numbers_an_added_shape() {
    const KEY_C: u32 = 0x00c1_0100;
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!((lfo.number_of(0, KEY_A), lfo.number_of(1, KEY_B)), (7, 8));
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!(
        (lfo.number_of(0, KEY_A), lfo.number_of(1, KEY_B)),
        (7, 8),
        "idempotent"
    );
    // module 0 is reloaded with a file that adds a shape
    settle(&mut lfo, &core, &[&[KEY_A, KEY_C], &[KEY_B]]);
    assert_eq!(
        (lfo.number_of(0, KEY_A), lfo.number_of(1, KEY_B)),
        (7, 8),
        "loaded shapes keep their numbers"
    );
    assert_eq!(
        lfo.number_of(0, KEY_C),
        9,
        "the added shape takes the next free number"
    );
}

/// The `lfo.shapes` writes the core saw.
fn saves(core: &FakeCore) -> usize {
    core.calls()
        .iter()
        .filter(|c| matches!(c, dkrt_test_support::Call::SettingsPut(k, _) if k == SETTING))
        .count()
}

#[test]
fn settle_saves_the_map_only_when_it_changed() {
    let (_g, core, mut lfo) = fresh();
    let before = saves(&core);
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!(saves(&core), before + 1, "a new map is saved");
    settle(&mut lfo, &core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!(saves(&core), before + 1, "the same keys give the same map");
    settle(&mut lfo, &core, &[&[KEY_A]]);
    assert_eq!(
        saves(&core),
        before + 1,
        "a key that left changes no number"
    );
}

#[test]
fn a_refused_save_is_tried_again_at_the_next_boot() {
    let (_g, core, mut lfo) = fresh();
    core.put_result.set(E_FORMAT);
    settle(&mut lfo, &core, &[&[KEY_A]]);
    let first = saves(&core);
    core.put_result.set(0);
    settle(&mut lfo, &core, &[&[KEY_A]]);
    assert_eq!(saves(&core), first + 1);
}

#[test]
fn a_module_that_never_attached_detaches_without_touching_another_modules_shapes() {
    let (_g, core, mut lfo) = fresh();
    settle(&mut lfo, &core, &[&[KEY_A]]);
    core.boot_index.set(0);
    load(&mut lfo, &core, 1, KEY_A, b"RAMP");
    detach(&mut lfo, &core, 5);
    detach(&mut lfo, &core, 5);
    assert_eq!((used(), owner_of(7)), (1, 1));
    assert!(graph::tiles_of(7).is_some());
}
