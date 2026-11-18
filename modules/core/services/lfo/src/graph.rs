//! `lfo` service, the shape graph: what the MOD page draws for a wave number of a loaded shape (docs/plans/extension-model.md E6 item 1b, finding 143 "Graph").
//!
//! The page does not draw a curve. Its cell blits `Bitmap` tiles of one period (28 x 15 pixels, 1 bit per pixel, column-major) from a `std::vector<Bitmap>` of four
//! tiles that it picks from a RAM table of seven vectors. A shape gets the same: one vector of four tiles, built by `publish` from the 32 samples and handed to the
//! cell by the UI-task hook at the vector pick (`lfo_ui_graph` in `dks/src/hooks-ui.s`, through [`lookup`]). The service never changes what the firmware keeps.
//!
//! Layout, as the cell reads it (32-bit words, big endian on the ColdFire): the vector header is `begin, end, end of storage` over four `Bitmap` objects of 28 bytes
//! (`+0` vtable, `+4` width 28, `+8` height 15, `+0xc` words per column 1, `+0x10` pixel words, `+0x14` mask words, `+0x18` owner byte 0: nothing frees them). A tile is 28 words,
//! one per column; row `r` (0 is the top) is bit `31 - r`; the 15 rows are bits 31 to 17. The mask is 28 words of `0xfffe0000`, shared by every tile of every shape.
//!
//! Which element the cell takes depends on the signs of the LFO speed and depth (the two values it splits at 63: finding 143). The four elements of every stock wave
//! are, with `v(t)` the wave: element 2 = `v(t)`, element 3 = `v(-t)` (time reversed), element 0 = `-v(t)`, element 1 = `-v(-t)`. Measured on TRI, SIN, SQR, SAW, EXPO and
//! RAMP; for an odd wave (TRI, SIN, SQR, SAW) `v(-t) = -v(t)`, so there are two pictures, and for the unipolar waves (EXPO, RAMP) four. A shape gets the four.
//!
//! The row of a sample `v` (Q31) is `7 - round(7 v)` (round half up): 0 for +1, 7 (the middle row) for 0, 14 for -1. A unipolar shape has rows 0 to 7 (and 7 to 14 mirrored), as the stock EXPO and RAMP.
//!
//! Publishing: a shape's graph is complete when its `valid` word is set, by one aligned 32-bit store after every other word. The hook reads `valid` first. The fields the cell
//! reads (width, height, pointers, the vector header) are written once and never change; a replaced shape only changes tile pixels, so a draw that races a reload shows
//! one torn picture and cannot fault. `retract` clears `valid` first.
use crate::records::{highest, interp, MAX_SHAPES, RANGE, SAMPLES};
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

/// Columns of a tile, one period of the wave.
pub const TILE_W: usize = 28;
/// The row of the value 0.
const MID: i32 = 7;
/// The mask word of every column: rows 0 to 14.
pub const MASK_WORD: u32 = 0xfffe_0000;
/// A jump of this many rows between neighbouring columns is drawn as a full vertical line in the later column (the stock tiles of SQR, SAW, EXPO and RAMP do it); a smaller step
/// gives each column the rows up to the middle of the step.
const JUMP: i32 = 7;

/// One tile: a pixel word per column.
pub type Tile = [u32; TILE_W];

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

/// The graph of one shape slot, in the layout the cell reads: the vector header first (the hook hands the cell a pointer to the slot).
#[derive(Clone, Copy)]
#[repr(C, align(4))]
struct Graph {
    /// the `std::vector<Bitmap>`: begin, end, end of storage
    vec: [u32; 3],
    /// not 0 once the graph is complete (the last word `publish` stores)
    valid: u32,
    /// 1 for a unipolar shape: the flag byte the cell reads for the number of tiles it draws
    unipolar: u32,
    /// four `Bitmap` objects of 28 bytes
    elems: [[u32; 7]; 4],
    tiles: [Tile; 4],
}

const NONE: Graph = Graph {
    vec: [0; 3],
    valid: 0,
    unipolar: 0,
    elems: [[0; 7]; 4],
    tiles: [[0; TILE_W]; 4],
};
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

fn graphs() -> *mut Graph {
    addr_of_mut!(GRAPHS) as *mut Graph
}

/// The graph slot `i` (`i < MAX_SHAPES`).
unsafe fn at(i: usize) -> *mut Graph {
    graphs().add(i)
}

/// Builds the graph of the shape in slot `i` and publishes it. The slot is free, so `valid` is 0 (it is cleared first all the same).
///
/// # Safety
/// `i < MAX_SHAPES`; one writer at a time (the service's apply and remove).
pub unsafe fn publish(i: usize, samples: &[i16; SAMPLES], unipolar: bool) {
    let g = at(i);
    write_volatile(addr_of_mut!((*g).valid), 0);
    compiler_fence(Ordering::Release);
    let tiles = build(samples);
    let mut k = 0;
    while k < 4 {
        let mut c = 0;
        while c < TILE_W {
            write_volatile(addr_of_mut!((*g).tiles[k][c]), tiles[k][c]);
            c += 1;
        }
        k += 1;
    }
    let mask = addr_of_mut!(MASK) as *mut u32;
    let mut c = 0;
    while c < TILE_W {
        write_volatile(mask.add(c), MASK_WORD);
        c += 1;
    }
    // the fields the cell reads are written once: the first publish of the slot (width 0 marks it unwritten)
    if read_volatile(addr_of_mut!((*g).elems[0][1])) == 0 {
        let mut k = 0;
        while k < 4 {
            let e = addr_of_mut!((*g).elems[k]) as *mut u32;
            write_volatile(e.add(1), TILE_W as u32);
            write_volatile(e.add(2), 15);
            write_volatile(e.add(3), 1);
            write_volatile(e.add(4), addr_of_mut!((*g).tiles[k]) as usize as u32);
            write_volatile(e.add(5), mask as usize as u32);
            write_volatile(e.add(6), 0);
            k += 1;
        }
        let begin = addr_of_mut!((*g).elems) as usize as u32;
        write_volatile(addr_of_mut!((*g).vec[0]), begin);
        write_volatile(addr_of_mut!((*g).vec[1]), begin + 4 * 28);
        write_volatile(addr_of_mut!((*g).vec[2]), begin + 4 * 28);
    }
    write_volatile(addr_of_mut!((*g).unipolar), unipolar as u32);
    compiler_fence(Ordering::Release);
    write_volatile(addr_of_mut!((*g).valid), 1);
}

/// Withdraws the graph of slot `i`: the hook gives the cell the stock vector from now on. The memory stays (it is the service's own, nothing frees it).
///
/// # Safety
/// `i < MAX_SHAPES`.
pub unsafe fn retract(i: usize) {
    write_volatile(addr_of_mut!((*at(i)).valid), 0);
    compiler_fence(Ordering::Release);
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
    if read_volatile(addr_of_mut!((*g).valid)) == 0 {
        return if wave <= highest() {
            STOCK_TRI
        } else {
            core::ptr::null()
        };
    }
    let mut k = 0;
    while k < 4 {
        let w = addr_of_mut!((*g).elems[k][0]);
        if read_volatile(w) != vtable {
            write_volatile(w, vtable);
        }
        k += 1;
    }
    write_volatile(flag, read_volatile(addr_of_mut!((*g).unipolar)) as u8);
    g as *const u8
}

/// The tiles of the graph of wave number `number` while it is published (what the cell would blit).
pub fn tiles_of(number: u32) -> Option<[Tile; 4]> {
    let i = slot(number)?;
    unsafe {
        let g = at(i);
        if read_volatile(addr_of_mut!((*g).valid)) == 0 {
            return None;
        }
        Some((*g).tiles)
    }
}

/// A word that changes with the published pictures of the slots (0 when none is published): the state view of the contract suite.
pub fn digest() -> u32 {
    let mut d = 0u32;
    let mut i = 0;
    while i < MAX_SHAPES {
        unsafe {
            let g = at(i);
            if read_volatile(addr_of_mut!((*g).valid)) != 0 {
                let mut k = 0;
                while k < 4 {
                    let mut c = 0;
                    while c < TILE_W {
                        d = d.rotate_left(5) ^ (*g).tiles[k][c] ^ (i as u32 + 1);
                        c += 1;
                    }
                    k += 1;
                }
            }
        }
        i += 1;
    }
    d
}
