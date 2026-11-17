//! Host tests of the `lfo` service over `FakeCore`: what it declares, how it numbers, checks, applies and removes `lfo_shape` records, how the interrupt
//! site's lookup reads the table, and that it ignores a record of another service. The statics of the crate (the shape table, the settled keys) are shared,
//! so each test takes `LOCK` and starts from an empty table.
//! The runtime's own tests (`handlers/lfo_tests.rs` in the runtime crate) drive the same service through the collector, the registry walkers and the planner.
use dkrt_abi::service::{check_step, ApplyCtx, CheckCtx, Declare, Service, SettleCtx};
use dkrt_sdk::records::{KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_sdk::*;
use dkrt_svc_lfo::records::{
    eval, interp, name_of, number_of, owner_of, shape, tri, unipolar_of, used, Lfo, MAX_SHAPES,
    NAME_LEN, RANGE, SAMPLES, SETTING,
};
use dkrt_test_support::FakeCore;
use std::sync::{Mutex, MutexGuard};
use std::vec::Vec;

static LOCK: Mutex<()> = Mutex::new(());

/// The lock, with an empty shape table and no settled keys.
fn fresh() -> (MutexGuard<'static, ()>, FakeCore) {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let core = FakeCore::new();
    for owner in 0..=8u8 {
        unsafe { <Lfo as Service<FakeCore>>::remove(&core, owner) };
    }
    unsafe { <Lfo as Service<FakeCore>>::settle(&core, &SettleCtx::new()) };
    assert_eq!(used(), 0);
    (g, core)
}

/// `settle` over modules that provide these shape keys (module `k` at selection index `k`).
fn settle(core: &FakeCore, modules: &[&[u32]]) {
    let mut sel = SettleCtx::new();
    sel.count = modules.len();
    for (k, keys) in modules.iter().enumerate() {
        sel.shape_nkeys[k] = keys.len() as u8;
        sel.shape_keys[k][..keys.len()].copy_from_slice(keys);
    }
    unsafe { <Lfo as Service<FakeCore>>::settle(core, &sel) };
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
    for (key, from) in shapes {
        s.extend_from_slice(&KIND_LFO_SHAPE.to_be_bytes());
        s.extend_from_slice(&LEN_LFO_SHAPE.to_be_bytes());
        s.extend_from_slice(&body(*key, b"TEST", *from));
    }
    s
}

fn check(core: &FakeCore, step: u32, sec: &[u8]) -> i32 {
    let c = CheckCtx {
        reg: sec.as_ptr(),
        image_size: 0x1000,
    };
    unsafe { <Lfo as Service<FakeCore>>::check(core, step, &c) }
}

fn apply(core: &FakeCore, owner: u8, kind: u32, body: &[u8]) -> i32 {
    let c = ApplyCtx {
        owner,
        reg: core::ptr::null(),
        base: 0,
    };
    unsafe { <Lfo as Service<FakeCore>>::apply_record(core, kind, body.as_ptr(), &c) }
}

const KEY_A: u32 = 0x00a1_0100;
const KEY_B: u32 = 0x00b2_0200;

#[test]
fn the_service_declares_its_one_kind_and_nothing_else() {
    let mut d = Declare::new();
    unsafe { <Lfo as Service<FakeCore>>::declare(&mut d) };
    let mut want = Declare::new();
    want.kind(KIND_LFO_SHAPE, LEN_LFO_SHAPE);
    assert!(d.same_as(&want));
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
    let (_g, core) = fresh();
    settle(&core, &[&[KEY_A]]);
    assert_eq!(number_of(0, KEY_A), 7, "the first key takes the first number");
    core.boot_index.set(0);
    let sec = section(&[(KEY_A, -1000)]);
    assert_eq!(check(&core, check_step::RECORDS, &sec), OK);
    assert_eq!(check(&core, check_step::ROWS, &sec), OK);
    assert_eq!(apply(&core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"RAMP", -1000)), OK);
    let s = shape(7).expect("apply_record kept the shape");
    assert_eq!((s[0], s[1], s[31]), (-1000, -900, 2100));
    assert_eq!((owner_of(7), used()), (1, 1));
    assert_eq!(name_of(7), Some(*b"RAMP"));
    assert!(!unipolar_of(7), "a shape with a negative sample is bipolar");
    unsafe { <Lfo as Service<FakeCore>>::remove(&core, 2) };
    assert_eq!(used(), 1, "another module's removal keeps the shape");
    unsafe { <Lfo as Service<FakeCore>>::remove(&core, 1) };
    assert_eq!((shape(7), owner_of(7), used()), (None, 0, 0));
}

#[test]
fn the_numbers_are_kept_in_the_settings_key_across_boots() {
    let (_g, core) = fresh();
    settle(&core, &[&[KEY_A], &[KEY_B]]);
    assert_eq!((number_of(0, KEY_A), number_of(1, KEY_B)), (7, 8));
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
    settle(&core, &[&[KEY_B]]);
    assert_eq!(number_of(0, KEY_B), 8);
}

#[test]
fn a_key_that_the_module_does_not_provide_or_a_bad_name_or_a_key_twice_is_refused() {
    let (_g, core) = fresh();
    settle(&core, &[&[KEY_A, 0x00c3_0300]]);
    core.boot_index.set(0);
    assert_eq!(
        check(&core, check_step::RECORDS, &section(&[(0, 0)])),
        E_FORMAT
    );
    assert_eq!(
        check(&core, check_step::RECORDS, &section(&[(0x0099_9900, 0)])),
        E_FORMAT,
        "a literal key that the KEYS entry does not list"
    );
    assert_eq!(
        check(&core, check_step::RECORDS, &section(&[(KEY_A, 0), (KEY_A, 1)])),
        E_TYPE_USED
    );
    assert_eq!(
        check(&core, check_step::RECORDS, &section(&[(KEY_A, 0), (0x00c3_0300, 1)])),
        OK
    );
    let mut sec = section(&[(KEY_A, 0)]);
    sec[16 + 8 + 4] = 0x07; // a control character in the name
    assert_eq!(check(&core, check_step::RECORDS, &sec), E_FORMAT);
}

#[test]
fn a_number_that_a_module_keeps_already_and_a_full_table_are_refused() {
    let (_g, core) = fresh();
    let keys: Vec<u32> = (0..MAX_SHAPES as u32).map(|i| 0x0010_0000 + (i << 8)).collect();
    settle(&core, &[&keys[..4], &keys[4..]]);
    core.boot_index.set(0);
    assert_eq!(apply(&core, 1, KIND_LFO_SHAPE, &body(keys[0], b"A   ", 0)), OK);
    assert_eq!(
        apply(&core, 2, KIND_LFO_SHAPE, &body(keys[0], b"B   ", 0)),
        E_TYPE_USED
    );
    assert_eq!(
        check(&core, check_step::ROWS, &section(&[(keys[0], 0)])),
        E_TYPE_USED
    );
    // a key that settle gave no number (a ninth key of the boot) has no place
    let nine: Vec<u32> = (0..MAX_SHAPES as u32 + 1).map(|i| 0x0020_0000 + (i << 8)).collect();
    settle(&core, &[&nine[..5], &nine[5..]]);
    core.boot_index.set(1);
    assert_eq!(number_of(1, nine[MAX_SHAPES]), 0);
    assert_eq!(
        check(&core, check_step::ROWS, &section(&[(nine[MAX_SHAPES], 0)])),
        E_TABLE_FULL
    );
    assert_eq!(
        apply(&core, 1, KIND_LFO_SHAPE, &body(nine[MAX_SHAPES], b"X   ", 0)),
        E_TABLE_FULL
    );
}

#[test]
fn a_record_of_another_service_and_the_other_check_steps_are_not_its_business() {
    let (_g, core) = fresh();
    assert_eq!(apply(&core, 1, 999, &body(KEY_A, b"TEST", 0)), OK);
    assert_eq!(used(), 0);
    let dup = section(&[(9, 0), (9, 1)]);
    assert_eq!(check(&core, check_step::COUNTS, &dup), OK);
    assert_eq!(check(&core, check_step::NATIVE, &dup), OK);
}

#[test]
fn the_writes_to_the_table_leave_the_interrupt_mask_released() {
    let (_g, core) = fresh();
    settle(&core, &[&[KEY_A]]);
    core.boot_index.set(0);
    assert_eq!(apply(&core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"RAMP", 0)), OK);
    assert_eq!(core.host.masked.get(), 0);
    unsafe { <Lfo as Service<FakeCore>>::remove(&core, 1) };
    assert_eq!(core.host.masked.get(), 0);
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
    let (_g, core) = fresh();
    settle(&core, &[&[KEY_A]]);
    core.boot_index.set(0);
    assert_eq!(
        eval(7, 0x4000_0000),
        tri(0x4000_0000),
        "no module keeps number 7 yet"
    );
    assert_eq!(apply(&core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"RAMP", -1000)), OK);
    assert_eq!(eval(7, 0), -1000 << 16);
    assert_eq!(eval(7, 1 << 26), -950 << 16);
    assert_eq!(eval(8, 1 << 26), tri(1 << 26), "number 8 has no shape");
    assert_eq!(eval(100, 0x4000_0000), tri(0x4000_0000), "a number outside the range");
    unsafe { <Lfo as Service<FakeCore>>::remove(&core, 1) };
    assert_eq!(eval(7, 0), tri(0), "an unloaded shape falls back");
}

#[test]
fn a_shape_without_a_negative_sample_is_unipolar() {
    let (_g, core) = fresh();
    settle(&core, &[&[KEY_A]]);
    core.boot_index.set(0);
    assert_eq!(apply(&core, 1, KIND_LFO_SHAPE, &body(KEY_A, b"UNI ", 0)), OK);
    assert!(unipolar_of(7));
    assert!(!unipolar_of(8));
}
