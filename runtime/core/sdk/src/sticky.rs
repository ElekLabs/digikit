//! Sticky numbers: the assignment loop of the machines service (`services/machines/src/assign.rs`), free of the card, the firmware and the debug log,
//! so it runs on the host. A service passes the range its numbers come from (`Range`) and its map; the loop knows no kind. The rules and the map
//! format are described in the service.
//!
//! A key is `hash24 << 8` (low byte 0); a map entry is `hash24 << 8 | number`, 0 = empty. A key with hash 0 is a pinned number and is not assigned
//! here. `assign` takes unique keys: the boot planner refuses a key that two tables provide or one table provides twice (`E_PLAN_KEY`).
//!
//! ColdFire rules (as in `store.rs`): no `match` on integers, no `core::fmt`.
use crate::logk;
use crate::SEL_MAX;

/// The numbers a service assigns: `lo ..= hi`, at most 32 of them (a number is a bit of a `u32` mask).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub lo: u32,
    pub hi: u32,
}

impl Range {
    pub fn contains(&self, t: u32) -> bool {
        t >= self.lo && t <= self.hi
    }
    fn bit(&self, t: u32) -> u32 {
        debug_assert!(
            self.hi >= self.lo && self.hi - self.lo < 32,
            "a range holds at most 32 numbers"
        );
        1 << (t - self.lo)
    }
}

/// Machines a boot can name: 8 selected modules with 8 machines each.
pub const MAX_KEYS: usize = SEL_MAX * 8;
const NONE: u32 = 0xffff_ffff;

/// A log event of the assignment; the runtime writes it to its debug log (`error`: an error event, else info level).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    /// `logk::MACHINE_TYPE_ASSIGNED` or `logk::MACHINE_TYPE_EVICTED`.
    pub kind: u16,
    pub error: bool,
    pub args: [u32; 5],
}

pub fn in_range(r: Range, t: u32) -> bool {
    r.contains(t)
}

/// Numbers referenced by the map, leaving out the entry of hash `except` (0: none left out).
pub fn referenced_but(r: Range, map: &[u32], except: u32) -> u32 {
    let mut m = 0;
    let mut i = 0;
    while i < map.len() {
        if map[i] != 0 && map[i] >> 8 != except {
            m |= r.bit(map[i] & 0xff);
        }
        i += 1;
    }
    m
}

/// Numbers referenced by the map.
pub fn referenced(r: Range, map: &[u32]) -> u32 {
    referenced_but(r, map, 0)
}

/// The lowest number of the range whose bit is not in `taken` (0: none).
pub fn lowest_free(r: Range, taken: u32) -> u32 {
    let mut t = r.lo;
    while t <= r.hi {
        if taken & r.bit(t) == 0 {
            return t;
        }
        t += 1;
    }
    0
}

/// Writes the map `map` into `out` (the same length) without entries that cannot be right: a number outside the range, no hash, a number or a hash
/// that an earlier entry has.
pub fn clean(r: Range, map: &[u32], out: &mut [u32]) {
    let mut seen = 0u32;
    let mut i = 0;
    while i < map.len() {
        out[i] = 0;
        let (h, t) = (map[i] >> 8, map[i] & 0xff);
        let mut dup_hash = false;
        let mut j = 0;
        while j < i {
            dup_hash |= out[j] != 0 && out[j] >> 8 == h;
            j += 1;
        }
        if h != 0 && r.contains(t) && seen & r.bit(t) == 0 && !dup_hash {
            out[i] = map[i];
            seen |= r.bit(t);
        }
        i += 1;
    }
}

pub fn find_hash(map: &[u32], h: u32) -> Option<usize> {
    let mut i = 0;
    while i < map.len() {
        if map[i] != 0 && map[i] >> 8 == h {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// The first empty slot, `map.len()` when the map is full.
pub fn first_empty(map: &[u32]) -> usize {
    let mut i = 0;
    while i < map.len() {
        if map[i] == 0 {
            return i;
        }
        i += 1;
    }
    map.len()
}

/// Is `h` the hash of one of the first `n` machine keys of this boot?
pub fn wanted(key: &[u32], n: usize, h: u32) -> bool {
    let mut i = 0;
    while i < n {
        if key[i] >> 8 == h {
            return true;
        }
        i += 1;
    }
    false
}

/// Assign the numbers of the first `n` `key`s (load order) into `ty` (0: none was free or the key is a pinned number), from the range `r`. `rec_map` is the
/// map the boot started from; `map` is the cleaned map on entry and the map to keep on return (both of one length). `log` gets every assignment and
/// eviction in order. Returns `dirty`: 1 when `map` differs from `rec_map`. The keys with a hash are unique (the planner's rule); a repeat is a bug
/// of the caller: it trips a debug assertion, and in a release build it gets no number (`ty` 0) and changes nothing.
pub fn assign<F: FnMut(&Event)>(
    r: Range,
    rec_map: &[u32],
    map: &mut [u32],
    key: &[u32],
    ty: &mut [u32],
    n: usize,
    log: &mut F,
) -> u32 {
    let mut used = 0u32;
    let mut e = 0;
    while e < n {
        ty[e] = 0;
        e += 1;
    }
    e = 0;
    while e < n {
        let k = key[e];
        let h = k >> 8;
        if h == 0 {
            e += 1;
            continue; // a pinned number (key = type) or nothing to assign
        }
        // a repeated key is a bug of the caller (the planner refuses it): debug builds stop, release builds give the repeat no number, so it cannot move
        // the number a saved key holds
        if repeated(key, e) {
            debug_assert!(false, "assign takes unique keys");
            e += 1;
            continue;
        }
        let mut t = 0;
        let mut sticky = 0;
        let mut rule = 0; // 0 lowest free number, 1 the map
        let at = find_hash(map, h);
        if let Some(i) = at {
            let old = map[i] & 0xff;
            if used & r.bit(old) == 0 {
                t = old;
                sticky = 1;
                rule = 1;
            }
        }
        if t == 0 {
            t = lowest_free(r, used | referenced(r, map));
            if t == 0 {
                // no number is unreferenced: drop the lowest entry of a key that is not in this boot
                let mut victim = NONE;
                let mut i = 0;
                while i < map.len() && victim == NONE {
                    if map[i] != 0 && !wanted(key, n, map[i] >> 8) {
                        victim = i as u32;
                    }
                    i += 1;
                }
                if victim != NONE {
                    let v = map[victim as usize];
                    log(&Event {
                        kind: logk::MACHINE_TYPE_EVICTED,
                        error: true,
                        args: [v >> 8, v & 0xff, 0, 0, 0],
                    });
                    map[victim as usize] = 0;
                    t = v & 0xff;
                }
            }
            if t != 0 {
                // record the entry: the slot the hash had, else the first empty one
                let slot = match at {
                    Some(i) => i,
                    None => first_empty(map),
                };
                if slot < map.len() {
                    map[slot] = h << 8 | t;
                }
            }
        }
        if t != 0 {
            used |= r.bit(t);
        }
        ty[e] = t;
        log(&Event {
            kind: logk::MACHINE_TYPE_ASSIGNED,
            error: false,
            args: [h, t, sticky, rule, 0],
        });
        e += 1;
    }
    let mut dirty = 0;
    let mut i = 0;
    while i < map.len() {
        dirty |= (map[i] != rec_map[i]) as u32;
        i += 1;
    }
    dirty
}

/// Whether key `e` is also among the keys before it.
fn repeated(key: &[u32], e: usize) -> bool {
    let mut j = 0;
    while j < e {
        if key[j] == key[e] {
            return true;
        }
        j += 1;
    }
    false
}

#[cfg(test)]
mod tests;
