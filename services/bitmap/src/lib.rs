//! Firmware `Bitmap` tiles and the vector header over them: what the `lfo` service hands the MOD page cell in place of a stock tile vector (docs/plans/extension-model.md E6e, finding 149 "Graph").
//!
//! Layout, as the cell reads it (32-bit words, big endian on the ColdFire): the vector header is `begin, end, end of storage` over four `Bitmap` objects of 28 bytes
//! (`+0` vtable, `+4` width 28, `+8` height 15, `+0xc` words per column 1, `+0x10` pixel words, `+0x14` mask words, `+0x18` owner byte 0: nothing frees them). A tile is 28 words,
//! one per column; row `r` (0 is the top) is bit `31 - r`; the 15 rows are bits 31 to 17. The mask is 28 words of [`MASK_WORD`], shared by every graph of the owner.
//!
//! Publishing: a graph is complete when its `valid` word is set, by one aligned 32-bit store after every other word. The reader looks at `valid` first. The fields the cell
//! reads (width, height, pointers, the vector header) are written once and never change; a replaced picture only changes tile pixels, so a draw that races a reload
//! shows one torn picture and cannot fault. [`retract`] clears `valid` first.
//!
//! The owner keeps the storage (a `static mut` array of [`Graph`] and one mask in `.bss`: code never copies a `Graph`, which would be a `.rodata` object the packer refuses
//! past its alignment) and passes pointers; this crate holds no state.
#![no_std]
#[cfg(test)]
extern crate std;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};

/// Columns of a tile, one period of the wave.
pub const TILE_W: usize = 28;
/// The mask word of every column: rows 0 to 14.
pub const MASK_WORD: u32 = 0xfffe_0000;

/// One tile: a pixel word per column.
pub type Tile = [u32; TILE_W];

/// The graph of one slot, in the layout the cell reads: the vector header first (the reader hands the cell a pointer to the slot).
#[derive(Clone, Copy)]
#[repr(C, align(4))]
pub struct Graph {
    /// the `std::vector<Bitmap>`: begin, end, end of storage
    vec: [u32; 3],
    /// not 0 once the graph is complete (the last word `publish` stores)
    valid: u32,
    /// 1 for a unipolar picture: the flag byte the cell reads for the number of tiles it draws
    unipolar: u32,
    /// four `Bitmap` objects of 28 bytes
    elems: [[u32; 7]; 4],
    tiles: [Tile; 4],
}

/// An empty (all zero, so `.bss`) graph, for the initialiser of the owner's array.
pub const NONE: Graph = Graph {
    vec: [0; 3],
    valid: 0,
    unipolar: 0,
    elems: [[0; 7]; 4],
    tiles: [[0; TILE_W]; 4],
};

/// Stores `tiles` into the graph `g` and publishes it. The graph is retracted first (`valid` is 0: the caller called [`retract`] before it built `tiles`, so a reader never sees a
/// graph that is half old and half new). `mask` is the owner's
/// shared mask of [`TILE_W`] words.
///
/// # Safety
/// `g` and `mask` are valid for writes; one writer at a time.
#[inline(always)]
pub unsafe fn publish(g: *mut Graph, mask: *mut u32, tiles: &[Tile; 4], unipolar: bool) {
    let mut k = 0;
    while k < 4 {
        let mut c = 0;
        while c < TILE_W {
            write_volatile(addr_of_mut!((*g).tiles[k][c]), tiles[k][c]);
            c += 1;
        }
        k += 1;
    }
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

/// Withdraws the graph `g`: a reader gives the cell the stock vector from now on. The memory stays (it is the owner's, nothing frees it).
///
/// # Safety
/// `g` is valid for writes.
#[inline(always)]
pub unsafe fn retract(g: *mut Graph) {
    write_volatile(addr_of_mut!((*g).valid), 0);
    compiler_fence(Ordering::Release);
}

/// Whether the graph `g` is not published (`valid` is 0).
///
/// # Safety
/// `g` is valid for reads.
#[inline(always)]
pub unsafe fn is_empty(g: *mut Graph) -> bool {
    read_volatile(addr_of_mut!((*g).valid)) == 0
}

/// The vtable word of element `k` (0 to 3) of the graph `g`: the reader copies the vtable of the stock tile vector there whenever it differs from what the word holds. Only that
/// reader writes it and only one task calls it, so no reader sees a half-written word.
///
/// # Safety
/// `g` is valid; `k < 4`.
#[inline(always)]
pub unsafe fn vtable_word(g: *mut Graph, k: usize) -> *mut u32 {
    addr_of_mut!((*g).elems[k][0])
}

/// The unipolar word of the graph `g`: 1 for a unipolar picture, the byte the cell reads for the number of tiles it draws.
///
/// # Safety
/// `g` is valid.
#[inline(always)]
pub unsafe fn unipolar_word(g: *mut Graph) -> *mut u32 {
    addr_of_mut!((*g).unipolar)
}

/// The `valid` word of the graph `g`: not 0 while it is published.
///
/// # Safety
/// `g` is valid.
#[inline(always)]
pub unsafe fn valid_word(g: *mut Graph) -> *mut u32 {
    addr_of_mut!((*g).valid)
}

/// The tiles of the graph `g` while it is published.
///
/// # Safety
/// `g` is valid for reads.
pub unsafe fn tiles_of(g: *mut Graph) -> Option<[Tile; 4]> {
    if is_empty(g) {
        return None;
    }
    Some((*g).tiles)
}

/// Folds the tiles of the graph `g` into `d` with the slot number `salt`, when it is published.
///
/// # Safety
/// `g` is valid for reads.
pub unsafe fn digest(mut d: u32, g: *mut Graph, salt: u32) -> u32 {
    if !is_empty(g) {
        let mut k = 0;
        while k < 4 {
            let mut c = 0;
            while c < TILE_W {
                d = d.rotate_left(5) ^ (*g).tiles[k][c] ^ salt;
                c += 1;
            }
            k += 1;
        }
    }
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiles() -> [Tile; 4] {
        let mut t = [[0u32; TILE_W]; 4];
        for (k, tile) in t.iter_mut().enumerate() {
            for (c, w) in tile.iter_mut().enumerate() {
                *w = 0x8000_0000 >> (k + c % 15);
            }
        }
        t
    }

    #[test]
    fn publish_writes_the_header_the_cell_reads_and_retract_withdraws_it() {
        let mut g = NONE;
        let mut mask = [0u32; TILE_W];
        let gp = &mut g as *mut Graph;
        let t = tiles();
        unsafe {
            assert!(is_empty(gp));
            publish(gp, mask.as_mut_ptr(), &t, true);
            assert!(!is_empty(gp));
            assert_eq!(tiles_of(gp), Some(t));
            assert_eq!(*unipolar_word(gp), 1);
            assert!(mask.iter().all(|w| *w == MASK_WORD));
            // vector header: begin, end and end of storage over four 28-byte Bitmaps
            let begin = gp.cast::<u8>().add(core::mem::offset_of!(Graph, elems)) as usize as u32;
            assert_eq!(g.vec, [begin, begin + 112, begin + 112]);
            for k in 0..4 {
                let e = g.elems[k];
                assert_eq!(&e[1..4], &[TILE_W as u32, 15, 1]);
                assert_eq!(e[4], addr_of_mut!(g.tiles[k]) as usize as u32);
                assert_eq!((e[5], e[6]), (mask.as_ptr() as usize as u32, 0));
            }
            retract(gp);
            assert!(is_empty(gp));
            assert_eq!(tiles_of(gp), None);
        }
    }

    #[test]
    fn the_vtable_word_of_each_element_is_addressable_and_the_digest_follows_the_tiles() {
        let mut g = NONE;
        let mut mask = [0u32; TILE_W];
        let gp = &mut g as *mut Graph;
        unsafe {
            assert_eq!(digest(7, gp, 1), 7, "an unpublished graph changes nothing");
            publish(gp, mask.as_mut_ptr(), &tiles(), false);
            for k in 0..4 {
                write_volatile(vtable_word(gp, k), 0x1000 + k as u32);
            }
            assert_eq!(g.elems.map(|e| e[0]), [0x1000, 0x1001, 0x1002, 0x1003]);
            let a = digest(0, gp, 1);
            assert_ne!(a, digest(0, gp, 2));
            assert_ne!(a, 0);
        }
    }
}
