//! `lfo` service, record handlers (interface B) and the table the LFO tick reads: the `lfo_shape` records of the `register` section. A record is `u32 key`
//! (the module's `DK_KEY_<ID>`: `hash24 << 8`), a 4-character name and 32 samples of `i16`, big-endian, after the 8-byte record header (`[records.kinds]` of
//! sdk/sdk.toml: 80 bytes).
//!
//! A shape is a value of the LFO WAVE parameter. The service gives each shape key a wave number from the sticky allocator (`dkrt_sdk::sticky`, the range
//! [`RANGE`], the settings key `lfo.shapes`), so a sound that names the number keeps its shape while the module stays in the map. The numbers stay below 0x80:
//! stock code clamps a wave byte of 7 to 127 to RAND and a negative byte to TRI (finding 149), so a number is safe in a sound when the service is not loaded.
//!
//! The table (`shapes`) is indexed by `number - RANGE.lo`. It is read in the interrupt of the LFO tick (`eval`), so it is written under the interrupt mask. One owner per number.
use crate::shapes::{cells_of, own_shapes, slot, used, Shape, SHAPES};
use crate::wave::{sync_range, wave_rows, WAVE};
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_abi::registry::each;
use dkrt_abi::service::{check_step, ApplyCtx, CheckCtx, Declare, Problem, Service, SettleCtx};
use dkrt_sdk::records::{KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_sdk::sticky::{assign, clean, Event, Range, MAX_KEYS};
use dkrt_sdk::*;

/// Samples of one shape.
pub const SAMPLES: usize = 32;
/// Shapes the table keeps.
pub const MAX_SHAPES: usize = 8;
/// The wave numbers of the shapes: 7 (the first above RAND) up, `MAX_SHAPES` of them, all below 0x80.
pub const RANGE: Range = Range {
    lo: 7,
    hi: 7 + MAX_SHAPES as u32 - 1,
};
/// The settings key of the sticky map: `MAX_SHAPES` words, big endian (`hash24 << 8 | number`).
pub const SETTING: &[u8] = b"lfo.shapes";
/// Bytes of the name of a shape.
pub const NAME_LEN: usize = 4;

/// The keys of the boot's selection and the numbers `settle` gave them.
struct Settled {
    n: usize,
    /// selection index of the module of each key
    at: [u8; MAX_KEYS],
    key: [u32; MAX_KEYS],
    num: [u32; MAX_KEYS],
    /// modules of the selection
    modules: usize,
}
static mut SETTLED: Settled = Settled {
    n: 0,
    at: [0; MAX_KEYS],
    key: [0; MAX_KEYS],
    num: [0; MAX_KEYS],
    modules: 0,
};

fn settled() -> &'static mut Settled {
    unsafe { &mut *addr_of_mut!(SETTLED) }
}

/// The wave number of the shape key `key` of the module at selection index `at` (0: none was given).
pub fn number_of(at: u32, key: u32) -> u32 {
    let s = unsafe { &*addr_of!(SETTLED) };
    let mut i = 0;
    while i < s.n {
        if s.key[i] == key && s.at[i] as u32 == at {
            return s.num[i];
        }
        i += 1;
    }
    0
}

fn words(raw: &[u8; MAX_SHAPES * 4]) -> [u32; MAX_SHAPES] {
    let mut map = [0u32; MAX_SHAPES];
    let mut i = 0;
    while i < MAX_SHAPES {
        map[i] = u32::from_be_bytes([raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]]);
        i += 1;
    }
    map
}

unsafe fn stored<C: CoreApi>(core: &C) -> [u32; MAX_SHAPES] {
    let mut raw = [0u8; MAX_SHAPES * 4];
    if core.settings_get(SETTING, &mut raw) == raw.len() as i32 {
        words(&raw)
    } else {
        [0; MAX_SHAPES]
    }
}

unsafe fn save<C: CoreApi>(core: &C, map: &[u32; MAX_SHAPES]) -> bool {
    let mut raw = [0u8; MAX_SHAPES * 4];
    let mut i = 0;
    while i < MAX_SHAPES {
        raw[4 * i..4 * i + 4].copy_from_slice(&map[i].to_be_bytes());
        i += 1;
    }
    core.settings_put(SETTING, &raw) == OK
}

/// Is `key` among the shape keys the module at selection index `at` provides (its `KEYS` entry)? True when no `settle` ran for that index (a host without a boot).
fn provided(at: u32, key: u32) -> bool {
    let s = unsafe { &*addr_of!(SETTLED) };
    if at as usize >= s.modules {
        return true;
    }
    let mut i = 0;
    while i < s.n {
        if s.at[i] as u32 == at && s.key[i] == key {
            return true;
        }
        i += 1;
    }
    false
}

/// The lfo service.
pub struct Lfo;

/// The rule for the shape records of one module (`validate::check_set`); a kind other than the shape record is accepted. The service's `validate_records` is this function,
/// and a host tool that has no core (`patchkit records --check`) calls it directly.
pub fn validate(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if kind == KIND_LFO_SHAPE {
        crate::validate::check_set(bodies, report);
    }
}

impl<C: CoreApi> Service<C> for Lfo {
    fn validate_records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
        validate(kind, bodies, report)
    }

    dkrt_abi::accepts_teardown!();

    unsafe fn declare(d: &mut Declare) {
        d.kind(KIND_LFO_SHAPE, LEN_LFO_SHAPE);
    }

    unsafe fn settle(core: &C, sel: &SettleCtx) {
        let s = settled();
        s.n = 0;
        s.modules = sel.count.min(dkrt_sdk::SEL_MAX);
        let from = stored(core);
        let mut map = [0u32; MAX_SHAPES];
        clean(RANGE, &from, &mut map);
        let mut k = 0;
        while k < s.modules {
            // the shape keys of the module (none for one the plan refused)
            let mut keys = [0u32; 8];
            let have = core.keys_of(k as u32, KIND_LFO_SHAPE, &mut keys);
            let mut i = 0;
            while i < have && s.n < MAX_KEYS {
                s.at[s.n] = k as u8;
                s.key[s.n] = keys[i];
                s.num[s.n] = 0;
                s.n += 1;
                i += 1;
            }
            k += 1;
        }
        let n = s.n;
        let dirty = assign(
            RANGE,
            &from,
            &mut map,
            &s.key,
            &mut s.num,
            n,
            &mut |_: &Event| {},
        );
        // a refused write is tried again at the next boot: the same map gives the same numbers
        if dirty != 0 {
            save(core, &map);
        }
    }

    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        if step == check_step::RECORDS {
            // the module's shapes meet the rule (`validate`: count, names, key once, samples), and each key is one the module's KEYS entry lists
            let mut bodies: [&[u8]; MAX_SHAPES + 1] = [&[]; MAX_SHAPES + 1];
            let mut n = 0;
            let mut r = OK;
            let at = core.boot_index();
            each(c.reg, |kind, b| {
                if kind == KIND_LFO_SHAPE {
                    if n <= MAX_SHAPES {
                        bodies[n] = core::slice::from_raw_parts(b, crate::validate::BODY_LEN);
                    }
                    n += 1;
                    if !provided(at, rd32(b)) {
                        r = E_FORMAT;
                    }
                }
            });
            let mut first = OK;
            let shown = if n < bodies.len() { n } else { bodies.len() };
            // more than MAX_SHAPES records: the count problem is the first
            // a module without shapes has nothing to check (the walker runs this step for every module)
            if n > 0 {
                crate::validate::check_set(&bodies[..shown], &mut |p: Problem| {
                    if first == OK {
                        first = p.code;
                    }
                });
            }
            if first != OK {
                first
            } else {
                r
            }
        } else if step == check_step::ROWS {
            // every key has a wave number, and the table has room (the shapes of the module being reloaded are given back before its new file loads). A number is never
            // held by another module: `settle` gives one number to each key, and the planner refuses a repeated key.
            let mut r = OK;
            let mut add = 0;
            let at = core.boot_index();
            each(c.reg, |kind, b| {
                if kind == KIND_LFO_SHAPE && r == OK {
                    if slot(number_of(at, rd32(b))).is_none() {
                        r = E_TABLE_FULL;
                    }
                    add += 1;
                }
            });
            if r == OK && used() - own_shapes(core.live_op() as u8) + add > MAX_SHAPES {
                r = E_TABLE_FULL;
            }
            // the WAVE rows must take the maximum (`sync_range`): the image has the registry's parameter table
            if r == OK && add > 0 && (*addr_of!(WAVE)).table != 0 && wave_rows(core).is_none() {
                r = E_NO_REGISTRY;
            }
            r
        } else {
            OK
        }
    }

    unsafe fn apply_record(core: &C, kind: u32, body: *const u8, c: &ApplyCtx) -> i32 {
        if kind != KIND_LFO_SHAPE {
            return OK;
        }
        let key = rd32(body);
        let i = match slot(number_of(core.boot_index(), key)) {
            Some(i) => i,
            None => return E_TABLE_FULL,
        };
        let t = &mut *addr_of_mut!(SHAPES);
        // defence in depth: `settle` gives each key its own number, so a slot is never held by another module; one that is (the module being reloaded still holds its own
        // until its teardown) is not overwritten
        if t[i].owner != 0 && t[i].owner != c.owner && t[i].owner as u32 != core.live_op() {
            return E_TYPE_USED;
        }
        let mut name = [0u8; NAME_LEN];
        name.copy_from_slice(core::slice::from_raw_parts(body.add(4), NAME_LEN));
        let mut samples = [0i16; SAMPLES];
        let mut s = 0;
        let mut neg = false;
        while s < SAMPLES {
            samples[s] = i16::from_be_bytes([*body.add(8 + 2 * s), *body.add(9 + 2 * s)]);
            neg |= samples[s] < 0;
            s += 1;
        }
        let sh = Shape {
            owner: c.owner,
            unipolar: !neg,
            name,
            cells: cells_of(&samples),
        };
        // the tick reads the entry in an interrupt: no half-written shape
        let sr = core.irq_mask();
        t[i] = sh;
        core.irq_restore(sr);
        // the page's picture (UI task): published last, after the table entry
        crate::graph::publish(i, &samples, !neg);
        // the WAVE maximum reaches the new number (UI task reads it; a write of one word)
        sync_range(core)
    }

    unsafe fn remove(core: &C, owner: u8) {
        let t = &mut *addr_of_mut!(SHAPES);
        let mut i = 0;
        while i < MAX_SHAPES {
            if t[i].owner == owner {
                // the hook gives the page the stock picture first
                crate::graph::retract(i);
                let sr = core.irq_mask();
                t[i].owner = 0;
                core.irq_restore(sr);
            }
            i += 1;
        }
        // the highest number may be lower now, or no shape is left: the stock pair goes back
        sync_range(core);
    }
}
