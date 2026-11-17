//! `lfo` service, record handlers (interface B) and the table the LFO tick reads: the `lfo_shape` records of the `register` section. A record is `u32 key`
//! (the module's `DK_KEY_<ID>`: `hash24 << 8`), a 4-character name and 32 samples of `i16`, big-endian, after the 8-byte record header (`[records.kinds]` of
//! sdk/sdk.toml: 80 bytes).
//!
//! A shape is a value of the LFO WAVE parameter. The service gives each shape key a wave number from the sticky allocator (`dkrt_sdk::sticky`, the range
//! [`RANGE`], the settings key `lfo.shapes`), so a sound that names the number keeps its shape while the module stays in the map. The numbers stay below 0x80:
//! stock code clamps a wave byte of 7 to 127 to RAND and a negative byte to TRI (finding 143), so a number is safe in a sound when the service is not loaded.
//!
//! The table is indexed by `number - RANGE.lo`. It is read in the interrupt of the LFO tick (`eval`), so it is written under the interrupt mask. One owner per number.
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_abi::registry::each;
use dkrt_abi::service::{check_step, ApplyCtx, CheckCtx, Declare, Service, SettleCtx};
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

#[derive(Clone, Copy)]
struct Shape {
    /// module record index + 1; 0: free
    owner: u8,
    /// the shape is unipolar: no sample is negative
    unipolar: bool,
    name: [u8; NAME_LEN],
    samples: [i16; SAMPLES],
}

const FREE: Shape = Shape {
    owner: 0,
    unipolar: false,
    name: [0; NAME_LEN],
    samples: [0; SAMPLES],
};
static mut SHAPES: [Shape; MAX_SHAPES] = [FREE; MAX_SHAPES];

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

fn slot(number: u32) -> Option<usize> {
    if RANGE.contains(number) {
        Some((number - RANGE.lo) as usize)
    } else {
        None
    }
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

/// The table index of `number` when a module keeps a shape there.
fn kept(t: &[Shape; MAX_SHAPES], number: u32) -> Option<usize> {
    match slot(number) {
        Some(i) if t[i].owner != 0 => Some(i),
        _ => None,
    }
}

/// The samples of the shape with wave number `number`, if a module keeps it.
pub fn shape(number: u32) -> Option<[i16; SAMPLES]> {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map(|i| t[i].samples)
}

/// The owner (module record index + 1) of wave number `number`; 0 when no module keeps it.
pub fn owner_of(number: u32) -> u8 {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map_or(0, |i| t[i].owner)
}

/// The name of the shape with wave number `number` (4 characters), if a module keeps it.
pub fn name_of(number: u32) -> Option<[u8; NAME_LEN]> {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map(|i| t[i].name)
}

/// Is the shape with wave number `number` unipolar (false for a number no module keeps: the fallback is TRI)?
pub fn unipolar_of(number: u32) -> bool {
    let t = unsafe { &*addr_of!(SHAPES) };
    kept(t, number).map_or(false, |i| t[i].unipolar)
}

/// Shapes in the table.
pub fn used() -> usize {
    let t = unsafe { &*addr_of!(SHAPES) };
    let mut n = 0;
    let mut i = 0;
    while i < MAX_SHAPES {
        if t[i].owner != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// The built-in triangle (the stock TRI function): the fallback of a number whose shape is not loaded. Full scale, 0 at phase 0.
pub fn tri(phase: u32) -> i32 {
    let d = phase.wrapping_add(0x4000_0000);
    let mask = if d >> 31 != 0 { 0xffff_ffffu32 } else { 0 };
    ((d << 1) ^ mask ^ 0x8000_0000) as i32
}

/// The value of 32 samples at a 32-bit phase, as signed Q31: the sample at the top 5 bits and the next, linearly interpolated with the next 15 bits (integer
/// arithmetic only; the product of the difference and the 15-bit fraction fits 32 bits).
pub fn interp(s: &[i16; SAMPLES], phase: u32) -> i32 {
    let i = (phase >> 27) as usize;
    let j = (i + 1) & (SAMPLES - 1);
    let f = ((phase >> 12) & 0x7fff) as i32;
    let a = s[i] as i32;
    let b = s[j] as i32;
    (a << 16).wrapping_add(((b - a) * f).wrapping_shl(1))
}

/// The value of the wave `number` at `phase`, signed Q31: the shape's samples interpolated, or TRI when no module keeps the number. This is what the
/// interrupt site of the LFO tick calls for a wave above RAND: it reads the table and writes nothing.
pub fn eval(number: u32, phase: u32) -> i32 {
    let t = unsafe { &*addr_of!(SHAPES) };
    match kept(t, number) {
        Some(i) => interp(&t[i].samples, phase),
        None => tri(phase),
    }
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

/// A name is 4 printable characters (space to tilde).
fn name_ok(b: *const u8) -> bool {
    let mut i = 0;
    while i < NAME_LEN {
        let c = unsafe { *b.add(4 + i) };
        if !(0x20..=0x7e).contains(&c) {
            return false;
        }
        i += 1;
    }
    true
}

/// The lfo service.
pub struct Lfo;

impl<C: CoreApi> Service<C> for Lfo {
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
            let mut i = 0;
            while i < sel.shape_nkeys[k] as usize && s.n < MAX_KEYS {
                s.at[s.n] = k as u8;
                s.key[s.n] = sel.shape_keys[k][i];
                s.num[s.n] = 0;
                s.n += 1;
                i += 1;
            }
            k += 1;
        }
        let n = s.n;
        let dirty = assign(RANGE, &from, &mut map, &s.key, &mut s.num, n, &mut |_: &Event| {});
        // a refused write is tried again at the next boot: the same map gives the same numbers
        if dirty != 0 {
            save(core, &map);
        }
    }

    unsafe fn check(core: &C, step: u32, c: &CheckCtx) -> i32 {
        if step == check_step::RECORDS {
            // each shape has a key that the module's KEYS entry lists, a printable name, and the key is once in the file
            let mut r = OK;
            let mut seen = [0u32; MAX_SHAPES];
            let mut n = 0;
            let at = core.boot_index();
            each(c.reg, |kind, b| {
                if kind == KIND_LFO_SHAPE && r == OK {
                    let id = rd32(b);
                    let mut k = 0;
                    while k < n {
                        if seen[k] == id {
                            r = E_TYPE_USED;
                        }
                        k += 1;
                    }
                    if id == 0 || !name_ok(b) || !provided(at, id) {
                        r = E_FORMAT;
                    } else if n < MAX_SHAPES {
                        seen[n] = id;
                        n += 1;
                    }
                }
            });
            r
        } else if step == check_step::ROWS {
            // every key has a wave number, its number is free, and the table has room
            let mut r = OK;
            let mut add = 0;
            let at = core.boot_index();
            each(c.reg, |kind, b| {
                if kind == KIND_LFO_SHAPE && r == OK {
                    match slot(number_of(at, rd32(b))) {
                        None => r = E_TABLE_FULL,
                        Some(i) => {
                            if (*addr_of!(SHAPES))[i].owner != 0 {
                                r = E_TYPE_USED;
                            }
                        }
                    }
                    add += 1;
                }
            });
            if r == OK && used() + add > MAX_SHAPES {
                r = E_TABLE_FULL;
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
        if t[i].owner != 0 {
            return E_TYPE_USED;
        }
        let mut sh = FREE;
        sh.owner = c.owner;
        sh.name
            .copy_from_slice(core::slice::from_raw_parts(body.add(4), NAME_LEN));
        let mut s = 0;
        let mut neg = false;
        while s < SAMPLES {
            sh.samples[s] = i16::from_be_bytes([*body.add(8 + 2 * s), *body.add(9 + 2 * s)]);
            neg |= sh.samples[s] < 0;
            s += 1;
        }
        sh.unipolar = !neg;
        // the tick reads the entry in an interrupt: no half-written shape
        let sr = core.irq_mask();
        t[i] = sh;
        core.irq_restore(sr);
        OK
    }

    unsafe fn remove(core: &C, owner: u8) {
        let t = &mut *addr_of_mut!(SHAPES);
        let mut i = 0;
        while i < MAX_SHAPES {
            if t[i].owner == owner {
                let sr = core.irq_mask();
                t[i] = FREE;
                core.irq_restore(sr);
            }
            i += 1;
        }
    }
}
