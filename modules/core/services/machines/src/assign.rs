//! Sticky machine type numbers (docs/design/module-sdk.md 11.5 D7, docs/design/digikit-registry.md "Machine keys").
//!
//! A run-time machine has no fixed type number. Its `.dkm` carries a machine key, `hash24 << 8` (the hash is the low 24 bits of FNV-1a 32 of
//! `<module id>:<machine id>`, the low byte is 0), and the service gives it a number in the run-time range when the boot starts. A key with hash 0 is a pinned number
//! (the low byte). The service remembers the numbers in a map of up to 8 entries (entry = `hash24 << 8 | type`, 0 = empty) in the settings key `machines.types`
//! (32 bytes, 8 words, big endian), so a machine keeps its number while it stays in the map, and a project that names the number keeps playing that machine.
//! Before this service owned it, the map was in the reserved bytes of the selection record: `legacy_state` is read when the settings key is not there yet,
//! the map is saved to the key as soon as it is read, and the old bytes are zeroed once the key holds it (the migration, one release).
//!
//! `settle` (`Service::settle`) runs once per boot, after the services are loaded and before the first module: it reads the machine keys of every selected module,
//! assigns all numbers in load order and leaves the table `resolve` answers from. Doing it for the whole selection first is what lets the eviction rule see which
//! map entries belong to modules that load in this boot. The map is saved at once, and only when it changed (`dirty`).
//!
//! Rules of the assignment, per machine in load order (`dkrt_sdk::sticky`):
//! 1. the map has an entry for its hash and that number is not taken in this boot: use it (sticky; logged as rule 1);
//! 2. else the lowest number that no map entry references and this boot has not given out; record the entry (rule 0);
//! 3. else drop the lowest-index entry whose hash is not among this boot's machines (logged), take its number, record the entry (rule 0);
//!
//! The keys of one boot are unique: the boot planner refuses a key that two modules provide or one module provides twice (`E_PLAN_KEY`), so the assignment has no rule for a repeat.
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_abi::service::SettleCtx;
use dkrt_sdk::sel::MAP_N;
use dkrt_sdk::sticky::{assign, clean, in_range, Event, Range, MAX_KEYS};
use dkrt_sdk::*;

/// The settings key of the map: `MAP_N` words, big endian.
pub const SETTING: &[u8] = b"machines.types";

/// The boot's assignment, peekable by the proof tools (`DKRT_ASSIGN`): `magic` "DKAS", `n` machines planned, `dirty` 1 when the map changed.
#[repr(C, align(4))]
pub struct State {
    magic: u32,
    n: usize,
    dirty: u32,
    /// The map as it is now (starts as the stored one, cleaned).
    map: [u32; MAP_N],
    /// Per planned machine: the selection index of its module, its key, its number (0: none was free).
    idx: [u32; MAX_KEYS],
    key: [u32; MAX_KEYS],
    ty: [u32; MAX_KEYS],
}

#[no_mangle]
pub static mut DKRT_ASSIGN: State = State {
    magic: 0,
    n: 0,
    dirty: 0,
    map: [0; MAP_N],
    idx: [0; MAX_KEYS],
    key: [0; MAX_KEYS],
    ty: [0; MAX_KEYS],
};

fn st() -> &'static mut State {
    unsafe { &mut *addr_of_mut!(DKRT_ASSIGN) }
}

/// The run-time machine types of the device (its profile's); a device without a profile has none (an empty range).
pub fn range<C: CoreApi>(core: &C) -> Range {
    match crate::profile(core) {
        Some(p) if p.dyn_types > 0 => Range {
            lo: p.dyn_type_first,
            hi: p.dyn_type_first + p.dyn_types - 1,
        },
        _ => Range { lo: 1, hi: 0 },
    }
}

/// Entries of the sticky map in use now (for CAPACITY).
pub fn map_used() -> u32 {
    let mut n = 0;
    let mut i = 0;
    while i < MAP_N {
        if st().map[i] != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// Whether the map holds machine type number `t`: an absent module of that number keeps a MISSING placeholder (a machine row and a parked voice row) while it does.
/// A pinned number is in no map entry.
pub fn holds(t: u32) -> bool {
    let mut i = 0;
    while i < MAP_N {
        let e = st().map[i];
        if e != 0 && e & 0xff == t {
            return true;
        }
        i += 1;
    }
    false
}

/// The hash of the machine that holds number `t` in the map (0: none).
pub fn hash_of(t: u32) -> u32 {
    let mut i = 0;
    while i < MAP_N {
        let e = st().map[i];
        if e != 0 && e & 0xff == t {
            return e >> 8;
        }
        i += 1;
    }
    0
}

/// The type number of map entry `i` (0 when the entry is empty).
pub fn number_at(i: usize) -> u32 {
    let e = st().map[i];
    if e == 0 {
        0
    } else {
        e & 0xff
    }
}

fn words(raw: &[u8; MAP_N * 4]) -> [u32; MAP_N] {
    let mut map = [0u32; MAP_N];
    let mut i = 0;
    while i < MAP_N {
        map[i] = u32::from_be_bytes([raw[4 * i], raw[4 * i + 1], raw[4 * i + 2], raw[4 * i + 3]]);
        i += 1;
    }
    map
}

/// The stored map: the settings key; while the key is not there, the bytes the selection record kept (`legacy`: true then).
unsafe fn stored<C: CoreApi>(core: &C) -> ([u32; MAP_N], bool) {
    let mut raw = [0u8; MAP_N * 4];
    if core.settings_get(SETTING, &mut raw) == raw.len() as i32 {
        return (words(&raw), false);
    }
    (words(&core.legacy_state()), true)
}

/// Write the map to the settings key; true when it is stored.
unsafe fn save<C: CoreApi>(core: &C, map: &[u32; MAP_N]) -> bool {
    let mut raw = [0u8; MAP_N * 4];
    let mut i = 0;
    while i < MAP_N {
        raw[4 * i..4 * i + 4].copy_from_slice(&map[i].to_be_bytes());
        i += 1;
    }
    core.settings_put(SETTING, &raw) == OK
}

/// Assign the numbers of every machine of the selected modules (`Service::settle`).
pub unsafe fn plan<C: CoreApi>(core: &C, sel: &SettleCtx) {
    let a = st();
    a.magic = 0x444b_4153; // "DKAS"
    a.n = 0;
    let (from, legacy) = stored(core);
    let r = range(core);
    clean(r, &from, &mut a.map);
    // the machines of the selection, in load order
    let mut k = 0;
    while k < sel.count {
        let mut i = 0;
        while i < sel.nkeys[k] as usize && a.n < MAX_KEYS {
            a.idx[a.n] = k as u32;
            a.key[a.n] = sel.keys[k][i];
            a.ty[a.n] = 0;
            a.n += 1;
            i += 1;
        }
        k += 1;
    }
    let n = a.n;
    a.dirty = assign(
        r,
        &from,
        &mut a.map,
        &a.key,
        &mut a.ty,
        n,
        &mut |ev: &Event| {
            let flags = if ev.error { logk::F_ERROR } else { LOG_INFO };
            core.log(ev.kind, flags, ev.args);
        },
    );
    // Save when the map changed or still comes from the old place. A refused write leaves `dirty` set and the old place as it is: the next boot reads the same map, assigns the
    // same numbers and tries again. Once the key holds the map the old bytes are zeroed, so that a later erase of the key cannot bring back a stale map; a refused zeroing
    // is tried again at every boot while the old bytes are not 0.
    if a.dirty != 0 || (legacy && from != [0; MAP_N]) {
        if save(core, &a.map) {
            a.dirty = 0;
        } else {
            a.dirty = 1;
            return;
        }
    }
    if core.legacy_state() != [0u8; 32] {
        core.legacy_state_clear();
    }
}

/// The type number for machine key `key` of the module being loaded (the selection index `CoreApi::boot_index`): the number `plan` assigned, a pinned number as it is
/// (when it is in the run-time range), 0 when there is none.
pub unsafe fn resolve<C: CoreApi>(core: &C, key: u32) -> u32 {
    if key >> 8 == 0 {
        return if in_range(range(core), key) { key } else { 0 }; // a pinned number (0: none)
    }
    let a = st();
    let at = core.boot_index();
    let mut i = 0;
    while i < a.n {
        if a.key[i] == key && a.idx[i] == at {
            return a.ty[i];
        }
        i += 1;
    }
    0
}

/// Host tests: give each of `keys` the run-time type number after `dkrt_sdk::sticky`'s first, as `plan` does for the module being loaded (`at`; none with `&[]`).
pub unsafe fn seed_for_test(keys: &[u32], at: u32) {
    let a = st();
    a.n = keys.len();
    for (i, k) in keys.iter().enumerate() {
        a.idx[i] = at;
        a.key[i] = *k;
        a.ty[i] = dkrt_abi::profiles::DT2.dyn_type_first + i as u32;
    }
}
