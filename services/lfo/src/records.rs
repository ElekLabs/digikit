//! `lfo` service, record handlers (interface B) and the table the LFO tick reads: the `lfo_shape` records of the `register` section. A record is `u32 key`
//! (the module's `DK_KEY_<ID>`: `hash24 << 8`), a 4-character name and 32 samples of `i16`, big-endian, after the 8-byte record header (`[records.kinds]` of
//! sdk/sdk.toml: 80 bytes).
//!
//! A shape is a value of the LFO WAVE parameter. The service gives each shape key a wave number from the sticky allocator (`dkrt_sdk::sticky`, the range
//! [`RANGE`], the settings key `lfo.shapes`), so a sound that names the number keeps its shape while the module stays in the map. The numbers stay below 0x80:
//! stock code clamps a wave byte of 7 to 127 to RAND and a negative byte to TRI (finding 149), so a number is safe in a sound when the service is not loaded.
//!
//! The table is indexed by `number - RANGE.lo`. It is read in the interrupt of the LFO tick (`eval`), so it is written under the interrupt mask. One owner per number.
use core::ptr::{addr_of, addr_of_mut};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_abi::registry::each;
use dkrt_abi::service::{check_step, ApplyCtx, CheckCtx, Declare, Problem, Service, SettleCtx};
use dkrt_sdk::records::{KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_sdk::sticky::{assign, clean, Event, Range, MAX_KEYS};
use dkrt_sdk::*;
use dkrt_svc_params::{dynreg, set_stock_range, stock_range};

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

/// One cell of a shape, as the tick reads it: the sample as Q31 and the step to the next sample (the next of sample 31 is sample 0). The step needs 17 bits, so it is an `i32`.
/// 4-byte aligned on purpose: the ColdFire GCC gives an `i32` an alignment of 2, which would put the cells of a shape at an offset of 6 and make the tick read them misaligned.
#[derive(Clone, Copy)]
#[repr(C, align(4))]
struct Cell {
    /// `samples[i] << 16`
    v: i32,
    /// `samples[(i + 1) & 31] - samples[i]`
    d: i32,
}

/// `repr(C)` with `owner` first: a loop over the table that compares the address of the `owner` of the element after the last one with a running pointer makes a relocated
/// word that points at the end of `SHAPES`, not past it (`shift_bss_words` of patchkit accepts a word up to the end of `.bss` only).
#[derive(Clone, Copy)]
#[repr(C)]
struct Shape {
    /// module record index + 1; 0: free
    owner: u8,
    /// the shape is unipolar: no sample is negative
    unipolar: bool,
    name: [u8; NAME_LEN],
    /// the delta table, built by `apply_record` from the record's samples
    cells: [Cell; SAMPLES],
}

const FREE: Shape = Shape {
    owner: 0,
    unipolar: false,
    name: [0; NAME_LEN],
    cells: [Cell { v: 0, d: 0 }; SAMPLES],
};
// FREE only initialises the table (zero: `.bss`). Code never copies it: a copy of a whole shape would be a 264-byte `.rodata` object aligned to 32, which the packer refuses
// (the arena's alignment is 16). A shape is freed by clearing `owner` alone: every reader tests it first.
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
    crate::route::shape_index(number)
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
    kept(t, number).map(|i| {
        let mut out = [0i16; SAMPLES];
        let mut k = 0;
        while k < SAMPLES {
            out[k] = (t[i].cells[k].v >> 16) as i16;
            k += 1;
        }
        out
    })
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

/// The highest wave number a loaded shape has (0 when none is loaded).
///
/// Not read under the interrupt mask, and no torn read is possible: `owner` is one byte, and the only writers (`apply_record`, `remove`) change a whole entry under the interrupt
/// mask, so a reader at task level sees each entry before or after a change, never half of one. A change that lands during the loop gives a number that held at some point
/// of the call; the callers (the WAVE maximum and the name formatter, both in the UI task) tolerate that.
pub fn highest() -> u32 {
    let t = unsafe { &*addr_of!(SHAPES) };
    let mut hi = 0;
    let mut i = 0;
    while i < MAX_SHAPES {
        if t[i].owner != 0 {
            hi = RANGE.lo + i as u32;
        }
        i += 1;
    }
    hi
}

/// The largest value of the LFO WAVE parameter (`wave << 8`) while shapes are loaded: the maximum of the WAVE rows follows the highest loaded wave number, so the
/// encoder reaches no number above it. 0 when no shape is loaded: the stock maximum stays (finding 149, "Direct readers of the parameter table").
pub fn wave_max() -> u32 {
    highest() << 8
}

/// The three WAVE rows of the parameter table (one for each LFO): the stock addresses of the table and of the rows, which the port file resolves (`cf.param.data.descriptor_table`,
/// `cf.lfo.data.wave_row.N`), and the pair (minimum, maximum) each row had before the first shape was loaded. Words 0: a host build with no firmware, which has no rows to write.
struct Wave {
    table: u32,
    row: [u32; 3],
    /// the rows hold the shape maximum; `stock` has what to put back
    armed: bool,
    stock: [(u32, u32); 3],
}
static mut WAVE: Wave = Wave {
    table: 0,
    row: [0; 3],
    armed: false,
    stock: [(0, 0); 3],
};

/// The firmware addresses of the WAVE rows (the service file's `setup`, from the words `bind` resolved): the stock table and the rows of the three LFOs. Not called by a host
/// build; with `table` 0 the service writes no row and refuses nothing.
pub fn set_firmware(table: u32, rows: [u32; 3]) {
    let w = unsafe { &mut *addr_of_mut!(WAVE) };
    w.table = table;
    w.row = rows;
    w.armed = false;
}

/// The pairs (minimum, maximum) the three WAVE rows hold now, or `None` when the image cannot take the maximum: it has no registry table, or a row is not a stock row of its
/// parameter table. Nothing is written.
unsafe fn wave_rows<C: CoreApi>(core: &C) -> Option<[(u32, u32); 3]> {
    let w = &*addr_of!(WAVE);
    let inf = dynreg(core);
    let mut now = [(0, 0); 3];
    let mut i = 0;
    while i < 3 {
        now[i] = stock_range(core, inf, w.table, w.row[i])?;
        i += 1;
    }
    Some(now)
}

/// Make the maximum of the WAVE rows in the RAM copy of the parameter table equal `wave_max()` while a shape is loaded, and put the stock pair back when the last one
/// is gone. The page and the encoder read the row through the firmware's range accessor, which reads the copy. `E_NO_REGISTRY` when the image cannot take it (`wave_rows`);
/// then nothing was written.
unsafe fn sync_range<C: CoreApi>(core: &C) -> i32 {
    let w = &mut *addr_of_mut!(WAVE);
    if w.table == 0 {
        return OK;
    }
    let now = match wave_rows(core) {
        Some(n) => n,
        None => return E_NO_REGISTRY,
    };
    let inf = dynreg(core);
    let max = wave_max();
    if !w.armed {
        if max == 0 {
            return OK;
        }
        w.stock = now;
        w.armed = true;
    }
    let mut i = 0;
    while i < 3 {
        let (min, top) = if max == 0 {
            w.stock[i]
        } else {
            (w.stock[i].0, max)
        };
        set_stock_range(core, inf, w.table, w.row[i], min, top);
        i += 1;
    }
    w.armed = max != 0;
    OK
}

/// Writes the name of the shape that the WAVE parameter value `value` (`wave << 8`; the stock formatter ignores the low byte) names, `width` characters at most (a name stops at
/// its first NUL), then a NUL, and returns the number of characters. A wave number that no module keeps but that is at or below [`highest`] (a gap the sticky allocator can
/// leave) gets `width` dashes ("----", "---" on the three-character formatter): the tick plays TRI for it, so the page says the number is empty. 0 when the number is above
/// the highest loaded one: nothing is written and the stock formatter prints its "ERR". `out` has room for `width + 1` bytes.
///
/// # Safety
/// `out` is valid for `width + 1` bytes.
pub unsafe fn name_into(value: i32, out: *mut u8, width: usize) -> usize {
    let wave = (value >> 8) as u32;
    let name = match name_of(wave) {
        Some(n) => n,
        None => {
            if wave < RANGE.lo || wave > highest() {
                return 0;
            }
            [b'-'; NAME_LEN]
        }
    };
    let cap = if width < NAME_LEN { width } else { NAME_LEN };
    let mut n = 0;
    while n < cap && name[n] != 0 {
        *out.add(n) = name[n];
        n += 1;
    }
    *out.add(n) = 0;
    n
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

/// Shapes that module record `owner - 1` keeps (0 when `owner` is 0: no live job runs).
fn own_shapes(owner: u8) -> usize {
    let t = unsafe { &*addr_of!(SHAPES) };
    let mut n = 0;
    let mut i = 0;
    while owner != 0 && i < MAX_SHAPES {
        if t[i].owner == owner {
            n += 1;
        }
        i += 1;
    }
    n
}

/// The built-in triangle (the stock TRI function): the fallback of a number whose shape is not loaded. Full scale, 0 at phase 0.
#[inline]
pub fn tri(phase: u32) -> i32 {
    let d = phase.wrapping_add(0x4000_0000);
    let mask = if d >> 31 != 0 { 0xffff_ffffu32 } else { 0 };
    ((d << 1) ^ mask ^ 0x8000_0000) as i32
}

/// The value of 32 samples at a 32-bit phase, as signed Q31: the sample at the top 5 bits and the next, linearly interpolated with the next 15 bits (integer
/// arithmetic only; the product of the difference and the 15-bit fraction fits 32 bits). This is the formula in its plain form, from the samples; `interp_cells` gives
/// the same value from the delta table that the tick reads.
///
/// No panic path: no bounds check and no overflow check. Both indexes are below `SAMPLES` by construction (`phase >> 27` is at most 31, and `j` is masked), and the
/// arithmetic wraps.
#[inline]
pub fn interp(s: &[i16; SAMPLES], phase: u32) -> i32 {
    let i = (phase >> 27) as usize;
    let j = (i + 1) & (SAMPLES - 1);
    let f = ((phase >> 12) & 0x7fff) as i32;
    // SAFETY: i <= 31 and j <= 31, `SAMPLES` is 32
    let (a, b) = unsafe { (*s.get_unchecked(i) as i32, *s.get_unchecked(j) as i32) };
    (a << 16).wrapping_add(b.wrapping_sub(a).wrapping_mul(f).wrapping_shl(1))
}

/// The delta table of 32 samples: `{ s[i] << 16, s[(i + 1) & 31] - s[i] }` for each `i`.
fn cells_of(s: &[i16; SAMPLES]) -> [Cell; SAMPLES] {
    let mut c = [Cell { v: 0, d: 0 }; SAMPLES];
    let mut i = 0;
    while i < SAMPLES {
        let a = s[i] as i32;
        let b = s[(i + 1) & (SAMPLES - 1)] as i32;
        c[i] = Cell {
            v: a << 16,
            d: b - a,
        };
        i += 1;
    }
    c
}

/// The same value as `interp`, from the delta table: one load of a cell, one multiply, one shift and one add. `d` is at most 17 bits and `f` 15, so the product fits an `i32`.
#[inline]
fn interp_cells(c: &[Cell; SAMPLES], phase: u32) -> i32 {
    let i = (phase >> 27) as usize;
    let f = ((phase >> 12) & 0x7fff) as i32;
    // SAFETY: i <= 31, `SAMPLES` is 32
    let cell = unsafe { c.get_unchecked(i) };
    cell.v.wrapping_add(cell.d.wrapping_mul(f).wrapping_shl(1))
}

/// The value of the wave `number` at `phase`, signed Q31: the shape's samples interpolated, or TRI when no module keeps the number. This is what the
/// interrupt site of the LFO tick calls for a wave above RAND: it reads the table and writes nothing, and it has no panic path (no bounds check: `number - RANGE.lo`
/// is below `MAX_SHAPES` before the table is read).
#[inline]
pub fn eval(number: u32, phase: u32) -> i32 {
    let idx = match crate::route::shape_index(number) {
        Some(i) => i,
        None => return tri(phase),
    };
    // SAFETY: idx < MAX_SHAPES, the length of SHAPES; the table is only read here
    let sh = unsafe { &*(addr_of!(SHAPES) as *const Shape).add(idx) };
    if sh.owner == 0 {
        return tri(phase);
    }
    interp_cells(&sh.cells, phase)
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
