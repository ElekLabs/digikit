//! `lfo` service, the shape graph: what the MOD page draws for a wave number of a loaded shape (docs/plans/extension-model.md E6 item 1b, finding 149 "Graph").
//!
//! The page does not draw a curve. Its cell blits `Bitmap` tiles of one period (28 x 15 pixels, 1 bit per pixel, column-major) from a `std::vector<Bitmap>` of four
//! tiles that it picks from a RAM table of seven vectors. A shape gets the same: one vector of four tiles, built by `publish` from the 32 samples and handed to the
//! cell by the UI-task hook at the vector pick (`lfo_ui_graph` in `dks/src/hooks-ui.s`, through [`lookup`]). The service never changes what the firmware keeps.
//!
//! The tile and vector layout the cell reads, the mask and the `valid` handshake of publishing are in `dkrt-svc-bitmap`; this module builds the four tiles of a shape and keeps the per-slot storage.
//!
//! Which element the cell takes depends on the signs of the LFO speed and depth (the two values it splits at 63: finding 149). The four elements of every stock wave
//! are, with `v(t)` the wave: element 2 = `v(t)`, element 3 = `v(-t)` (time reversed), element 0 = `-v(t)`, element 1 = `-v(-t)`. Measured on TRI, SIN, SQR, SAW, EXPO and
//! RAMP; for an odd wave (TRI, SIN, SQR, SAW) `v(-t) = -v(t)`, so there are two pictures, and for the unipolar waves (EXPO, RAMP) four. A shape gets the four.
//!
//! The row of a sample `v` (Q31) is `7 - round(7 v)` (round half up): 0 for +1, 7 (the middle row) for 0, 14 for -1. A unipolar shape has rows 0 to 7 (and 7 to 14 mirrored), as the stock EXPO and RAMP.
//!
use crate::records::{highest, interp, MAX_SHAPES, RANGE, SAMPLES};
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use dkrt_svc_bitmap as bitmap;
use dkrt_svc_bitmap::{Graph, NONE};
pub use dkrt_svc_bitmap::{Tile, MASK_WORD, TILE_W};

/// The row of the value 0.
const MID: i32 = 7;
/// A jump of this many rows between neighbouring columns is drawn as a full vertical line in the later column (the stock tiles of SQR, SAW, EXPO and RAMP do it); a smaller step
/// gives each column the rows up to the middle of the step.
const JUMP: i32 = 7;

/// `2^32 / 28`, rounded down: the phase step of a tile column (27 columns stay below 2^32).
const COLUMN_PHASE: u32 = 153_391_689;

/// The row of Q31 sample `v`: `7 - round(7 v)` with round half up (an arithmetic shift), 0 to 14.
fn row_of(v: i32) -> i32 {
    // Q31 -> Q15 first: 7 * v does not fit 32 bits otherwise
    let t = ((v >> 16) * 7 + (1 << 14)) >> 15;
    let t = if t > MID {
        MID
    } else if t < -MID {
        -MID
    } else {
        t
    };
    MID - t
}

/// The row of each column: the shape at the phase of the column (`reversed`: at the phase of the mirrored time, `1 - t`).
fn rows(samples: &[i16; SAMPLES], reversed: bool) -> [i32; TILE_W] {
    let mut r = [0i32; TILE_W];
    let mut c = 0;
    while c < TILE_W {
        let k = if reversed { (TILE_W - c) % TILE_W } else { c };
        r[c] = row_of(interp(samples, (k as u32).wrapping_mul(COLUMN_PHASE)));
        c += 1;
    }
    r
}

/// The pixel words of a row curve. The column before column 0 is column 27 (the tile repeats).
fn columns(rows: &[i32; TILE_W]) -> Tile {
    let mut t = [0u32; TILE_W];
    let mut c = 0;
    while c < TILE_W {
        let r = rows[c];
        let p = rows[(c + TILE_W - 1) % TILE_W];
        // the rows of this column: from its own row towards the previous one, up to the middle of the step (a tie goes to this column); a jump fills the whole step
        let (lo, hi) = if (r - p).abs() >= JUMP {
            (r.min(p), r.max(p))
        } else if p > r {
            (r, (r + p) / 2)
        } else {
            ((r + p + 1) / 2, r)
        };
        let mut k = lo;
        while k <= hi {
            t[c] |= 1 << (31 - k);
            k += 1;
        }
        c += 1;
    }
    t
}

/// The four tiles of a shape in the order of the elements of the vector: element 0 `-v(t)`, element 1 `-v(-t)`, element 2 `v(t)`, element 3 `v(-t)`.
pub fn build(samples: &[i16; SAMPLES]) -> [Tile; 4] {
    let fwd = rows(samples, false);
    let rev = rows(samples, true);
    let mut mf = fwd;
    let mut mr = rev;
    let mut c = 0;
    while c < TILE_W {
        mf[c] = 2 * MID - fwd[c];
        mr[c] = 2 * MID - rev[c];
        c += 1;
    }
    [columns(&mf), columns(&mr), columns(&fwd), columns(&rev)]
}

// zero: `.bss`. Code never copies it (a copy would be a `.rodata` object, which the packer refuses past its alignment): a slot is rewritten word by word.
static mut GRAPHS: [Graph; MAX_SHAPES] = [NONE; MAX_SHAPES];
static mut MASK: [u32; TILE_W] = [0; TILE_W];

fn slot(number: u32) -> Option<usize> {
    if RANGE.contains(number) {
        Some((number - RANGE.lo) as usize)
    } else {
        None
    }
}

/// The graph slot `i` (`i < MAX_SHAPES`).
unsafe fn at(i: usize) -> *mut Graph {
    (addr_of_mut!(GRAPHS) as *mut Graph).add(i)
}

/// Builds the graph of the shape in slot `i` and publishes it (the tile store and handshake are `dkrt_svc_bitmap::publish`).
///
/// # Safety
/// `i < MAX_SHAPES`; one writer at a time (the service's apply and remove).
pub unsafe fn publish(i: usize, samples: &[i16; SAMPLES], unipolar: bool) {
    let g = at(i);
    bitmap::retract(g);
    bitmap::publish(g, addr_of_mut!(MASK) as *mut u32, &build(samples), unipolar);
}

/// Withdraws the graph of slot `i`: the hook gives the cell the stock vector from now on.
///
/// # Safety
/// `i < MAX_SHAPES`.
pub unsafe fn retract(i: usize) {
    bitmap::retract(at(i));
}

/// The result of [`lookup`] for a gap number: the hook sets the stock table offset of the TRI tile vector (0), since the tick plays TRI for a number with no shape. Not a valid address.
pub const STOCK_TRI: *const u8 = 1 as *const u8;

/// What the hook at the vector pick of the cell calls (UI task): the pointer to the vector header of the graph for wave `wave` (the cell's unclamped wave argument), with the
/// unipolar byte stored at `flag`; [`STOCK_TRI`] when `wave` is a gap (no module keeps it, and it is at or below the highest kept number); null otherwise (the hook then leaves the stock vector).
///
/// `vtable` is the vtable word of the stock tile vector the hook read (the first word of its first `Bitmap`): it is copied into the four elements whenever it differs from what they hold. Only this
/// function writes it and only the UI task calls it, so no reader sees a half-written word. A `vtable` of 0 gives null.
///
/// # Safety
/// `flag` is valid for one byte (the cell's frame).
pub unsafe fn lookup(wave: u32, vtable: u32, flag: *mut u8) -> *const u8 {
    let i = match slot(wave) {
        Some(i) => i,
        None => return core::ptr::null(),
    };
    let g = at(i);
    if vtable == 0 {
        return core::ptr::null();
    }
    if read_volatile(bitmap::valid_word(g)) == 0 {
        return if wave <= highest() {
            STOCK_TRI
        } else {
            core::ptr::null()
        };
    }
    let mut k = 0;
    while k < 4 {
        let w = bitmap::vtable_word(g, k);
        if read_volatile(w) != vtable {
            write_volatile(w, vtable);
        }
        k += 1;
    }
    write_volatile(flag, read_volatile(bitmap::unipolar_word(g)) as u8);
    g as *const u8
}

/// The tiles of the graph of wave number `number` while it is published (what the cell would blit).
pub fn tiles_of(number: u32) -> Option<[Tile; 4]> {
    let i = slot(number)?;
    unsafe { bitmap::tiles_of(at(i)) }
}

/// A word that changes with the published pictures of the slots (0 when none is published): the state view of the contract suite.
pub fn digest() -> u32 {
    let mut d = 0u32;
    let mut i = 0;
    while i < MAX_SHAPES {
        d = unsafe { bitmap::digest(d, at(i), i as u32 + 1) };
        i += 1;
    }
    d
}
