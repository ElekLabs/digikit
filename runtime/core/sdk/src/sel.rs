//! The selection record (docs/design/digikit-store.md): one sector, big endian, in two copies at the end of the store range.
// The selection (what the next boot loads): `SEL_MAX` (`[limits.max] modules` of sdk/sdk.toml, the record's format), `SEL_PATH` and `DkrtSelection` are in
// `gen_abi`, as are `SAFE_*`, `SAFE_AFTER`, `STABLE_BLOCKS` and `DkrtBootInfo`.

use super::*;

pub const MAGIC: u32 = 0x444b_534c; // "DKSL"
pub const VERSION: u32 = 1;
// The absolute sectors of the two copies are `store::Layout::sel_a` and `sel_b`: the end of the store area, which differs per device.

/// `Record::flags` bit 0: start in safe mode once. Bit 1 is reserved (it was the debug-off flag; the runtime ignores it and does not write it).
pub const FLAG_SAFE_ONCE: u32 = 1;

#[derive(Clone, Copy)]
pub struct Record {
    pub seq: u32,
    pub boot_counter: u32,
    pub flags: u32,
    pub count: u32,
    /// The stable period of this card in DSP blocks (bytes 24 to 27 of the record, reserved zero before): 0 takes `STABLE_BLOCKS`. A
    /// setting for tests and for people who debug a module; nothing in the menu writes it.
    pub stable_blocks: u32,
    /// The sticky machine types (docs/design/module-sdk.md 11.5 D7): up to 8 entries, each `hash24 << 8 | type`, 0 = empty. Entry 0 is
    /// bytes 28 to 31 of the record, entries 1 to 7 bytes 480 to 507.
    pub map: [u32; MAP_N],
    pub paths: [[u8; SEL_PATH]; SEL_MAX],
}

/// Entries of the sticky type map.
pub const MAP_N: usize = crate::gen_sdk::limits::MAP_N; // `[limits.max] map_entries` of sdk/sdk.toml

/// Byte offset of map entry `i`.
pub const fn map_at(i: usize) -> usize {
    if i == 0 {
        28
    } else {
        480 + 4 * (i - 1)
    }
}

/// The record a new selection writes over `cur`: the sequence number goes up, the boot counter, the stable period and the sticky type map are
/// kept (the map survives a new selection), the flags keep only `FLAG_SAFE_ONCE`.
pub fn next_record(
    cur: &Record,
    flags: u32,
    count: u32,
    paths: &[[u8; SEL_PATH]; SEL_MAX],
) -> Record {
    let mut r = Record::empty();
    r.seq = cur.seq.wrapping_add(1);
    r.boot_counter = cur.boot_counter;
    r.stable_blocks = cur.stable_blocks;
    r.map = cur.map;
    r.flags = flags & FLAG_SAFE_ONCE;
    r.count = count;
    let mut k = 0;
    while k < count as usize && k < SEL_MAX {
        r.paths[k] = paths[k];
        k += 1;
    }
    r
}

/// `E_SEL_COUNT` when a selection names more than `SEL_MAX` paths, else `OK`.
pub fn check_count(count: u32) -> i32 {
    if count as usize > SEL_MAX {
        E_SEL_COUNT
    } else {
        OK
    }
}

/// `check_count` for `CMD_SELECT_ONE`, whose copy index must be 0 or 1 (a bad index has the same code).
pub fn check_one(count: u32, copy: u32) -> i32 {
    if copy > 1 {
        E_SEL_COUNT
    } else {
        check_count(count)
    }
}

/// The mailbox list of paths takes path number `n` (0-based): `E_SEL_COUNT` when `SEL_MAX` paths are already there.
pub fn check_slot(n: usize) -> i32 {
    if n >= SEL_MAX {
        E_SEL_COUNT
    } else {
        OK
    }
}

impl Record {
    pub const fn empty() -> Record {
        Record {
            seq: 0,
            boot_counter: 0,
            flags: 0,
            count: 0,
            stable_blocks: 0,
            map: [0; MAP_N],
            paths: [[0; SEL_PATH]; SEL_MAX],
        }
    }
}

pub fn crc32(p: &[u8]) -> u32 {
    let mut crc = 0xffff_ffffu32;
    for &b in p {
        crc ^= b as u32;
        let mut k = 0;
        while k < 8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
            k += 1;
        }
    }
    !crc
}

fn put(b: &mut [u8], off: usize, v: u32) {
    b[off] = (v >> 24) as u8;
    b[off + 1] = (v >> 16) as u8;
    b[off + 2] = (v >> 8) as u8;
    b[off + 3] = v as u8;
}
fn get(b: &[u8], off: usize) -> u32 {
    ((b[off] as u32) << 24)
        | ((b[off + 1] as u32) << 16)
        | ((b[off + 2] as u32) << 8)
        | (b[off + 3] as u32)
}

pub fn encode(r: &Record, out: &mut [u8; 512]) {
    let mut i = 0;
    while i < 512 {
        out[i] = 0;
        i += 1;
    }
    put(out, 0, MAGIC);
    put(out, 4, VERSION);
    put(out, 8, r.seq);
    put(out, 12, r.boot_counter);
    put(out, 16, r.flags);
    put(out, 20, r.count);
    put(out, 24, r.stable_blocks);
    let mut m = 0;
    while m < MAP_N {
        put(out, map_at(m), r.map[m]);
        m += 1;
    }
    let mut k = 0;
    while k < SEL_MAX {
        let mut j = 0;
        while j < SEL_PATH {
            out[32 + k * SEL_PATH + j] = r.paths[k][j];
            j += 1;
        }
        k += 1;
    }
    let c = crc32(&out[..508]);
    put(out, 508, c);
}

/// A valid record: magic, version, CRC, count, every used path NUL terminated.
pub fn decode(b: &[u8; 512]) -> Option<Record> {
    if get(b, 0) != MAGIC || get(b, 4) != VERSION || get(b, 508) != crc32(&b[..508]) {
        return None;
    }
    let count = get(b, 20);
    if count as usize > SEL_MAX {
        return None;
    }
    let mut r = Record::empty();
    r.seq = get(b, 8);
    r.boot_counter = get(b, 12);
    r.flags = get(b, 16);
    r.count = count;
    r.stable_blocks = get(b, 24);
    let mut m = 0;
    while m < MAP_N {
        r.map[m] = get(b, map_at(m));
        m += 1;
    }
    let mut k = 0;
    while k < SEL_MAX {
        let mut j = 0;
        let mut nul = false;
        while j < SEL_PATH {
            let c = b[32 + k * SEL_PATH + j];
            r.paths[k][j] = c;
            nul |= c == 0;
            j += 1;
        }
        if (k as u32) < count && (!nul || r.paths[k][0] == 0) {
            return None;
        }
        k += 1;
    }
    Some(r)
}
