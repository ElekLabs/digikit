//! The pool table: what the calls of a module read and change at once (`dk_sample_pool_add`, `_set`, `_clear`, `_status`, `_info`, `_path`), kept outside the owner object.
//!
//! Those calls come from the MIDI task and the UI task. The UI task may be inside the owner (a tick, a peak window, a release) when the MIDI task preempts it, or a callback of
//! a module in the UI task may call in: the owner's guard refuses that call (`E_CONTEXT`). The calls answer at once from this table instead, in a section with interrupts masked
//! ([`Shared`]): per entry the status, the stored path with its key, the data of a READY entry (`dk_sample_info`), and per client whether it is open.
//!
//! The owner (`Samples`) keeps the state machine of the tracks (`dkrt_sdk::sample_tracks`: phases, references, the load) and the heavy work. It meets the table in short sections only:
//! - Requests: a call writes the entry's status and path here (`QUEUED` for add and set, `UNUSED` for clear: the result of the call and what the next `status` shows) and leaves a
//!   request. At the start of its tick (and before a loss of the backing) the owner takes each request out ([`Tracks::intake`](crate::tracks::Tracks)) and applies it to the
//!   state machine as `select` or `unbind`, so the machine changes at the same place as before, in the UI task.
//! - Publication: after a tick, a loss or a release the owner copies the status of each entry and the info of each READY entry here ([`Tracks::mirror`](crate::tracks::Tracks)),
//!   except for an entry with a request waiting: the newer request wins, so a status the owner computed for an older one is never written over it.
//!
//! Sections are bounded: the longest is the scan of `add` (16 entries, one compare of the 32-bit key each, and one byte compare of at most `PATH` bytes where keys are equal, plus
//! the copy of the path into the entry), `mirror` (32 entries, stores only) or `take_request` (32 entries and one path copy). No I/O, no call out, no allocation.
use crate::files::{root_of, rooted_path};
use crate::tracks::{CLIENTS, ENTRIES};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::shared::Shared;
use dkrt_sdk::sample_tracks::{path_key, pool_pick, unrooted, Info, Pick, PATH, READY, UNUSED};
use dkrt_sdk::{E_CONTRACT, E_NOT_FOUND, E_POOL, E_SEL_PATH};

/// What the owner still has to do for an entry (the last request wins).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Req {
    None,
    /// Queue a load of the stored path (`select`).
    Select,
    /// Close the entry and drop its references (`unbind`).
    Unbind,
}

#[derive(Clone, Copy)]
struct Slot {
    status: i32,
    req: Req,
    key: u32,
    plen: usize,
    path: [u8; PATH],
    info: Info,
}

const SLOT0: Slot = Slot {
    status: UNUSED,
    req: Req::None,
    key: 0,
    plen: 0,
    path: [0; PATH],
    info: Info {
        frames: 0,
        rate: 0,
        channels: 0,
        token: 0,
    },
};

/// The table. All-zero bytes are the empty table, so the static is `.bss`.
pub struct PoolTable {
    /// Is a backing adapter installed? Without one `add` and `set` refuse (`E_CONTRACT`).
    backed: bool,
    open: [bool; CLIENTS],
    slots: [[Slot; ENTRIES]; CLIENTS],
}

/// The table of the service file: one cell, a static of the service (it holds no owner state). `Samples` takes it by reference, so a host test gives its own.
pub type PoolCell = Shared<PoolTable>;

/// The cell of the service file; `Provider::start_in` empties it.
pub static POOL_TABLE: PoolCell = PoolCell::new(PoolTable::new());

/// One entry's published state, as the owner copies it from the state machine.
#[derive(Clone, Copy)]
pub(crate) struct Snap {
    pub(crate) status: i32,
    pub(crate) info: Info,
}

impl PoolTable {
    pub const fn new() -> PoolTable {
        PoolTable {
            backed: false,
            open: [false; CLIENTS],
            slots: [[SLOT0; ENTRIES]; CLIENTS],
        }
    }

    /// The client of a handle if its table is open.
    fn client(&self, handle: u32) -> Result<usize, i32> {
        let h = handle as usize;
        if h == 0 || h > CLIENTS || !self.open[h - 1] {
            return Err(E_NOT_FOUND);
        }
        Ok(h - 1)
    }

    /// Store `path` as the selection of entry `k` and leave the request.
    fn select(&mut self, c: usize, k: usize, key: u32, path: &[u8]) {
        let s = &mut self.slots[c][k];
        s.path[..path.len()].copy_from_slice(path);
        s.plen = path.len();
        s.key = key;
        s.status = dkrt_sdk::sample_tracks::QUEUED;
        s.req = Req::Select;
    }

    // ---- the calls of a module ---------------------------------------------------------------------------------------------------

    fn add(&mut self, c: usize, key: u32, path: &[u8]) -> i32 {
        let pick = pool_pick(
            key,
            path,
            self.slots[c]
                .iter()
                .map(|s| (s.status, s.key, &s.path[..s.plen])),
        );
        match pick {
            Pick::Same(k) => k as i32,
            Pick::Again(k) | Pick::Free(k) => {
                self.select(c, k, key, path);
                k as i32
            }
            Pick::Full => E_POOL,
        }
    }

    fn set(&mut self, c: usize, index: u32, key: u32, path: &[u8]) -> i32 {
        if index as usize >= ENTRIES {
            return E_CONTRACT;
        }
        self.select(c, index as usize, key, path);
        0
    }

    fn clear(&mut self, c: usize, index: u32) -> i32 {
        if index as usize >= ENTRIES {
            return E_CONTRACT;
        }
        let s = &mut self.slots[c][index as usize];
        s.status = UNUSED;
        s.req = Req::Unbind;
        0
    }

    fn status(&self, c: usize, index: u32) -> i32 {
        if index as usize >= ENTRIES {
            return E_CONTRACT;
        }
        self.slots[c][index as usize].status
    }

    fn info(&self, c: usize, index: u32) -> Result<Info, i32> {
        let s = self.status(c, index);
        if s != READY {
            return Err(if s < 0 { s } else { E_NOT_FOUND });
        }
        Ok(self.slots[c][index as usize].info)
    }

    /// The stored path of the entry into `out`; its length. Every state but UNUSED has one (the sticky error of a failed select still names its file).
    fn path(&self, c: usize, index: u32, out: &mut [u8; PATH]) -> Result<usize, i32> {
        if index as usize >= ENTRIES {
            return Err(E_CONTRACT);
        }
        let s = &self.slots[c][index as usize];
        if s.status == UNUSED {
            return Err(E_NOT_FOUND);
        }
        out[..s.plen].copy_from_slice(&s.path[..s.plen]);
        Ok(s.plen)
    }

    // ---- the owner ---------------------------------------------------------------------------------------------------------------

    /// The next request, oldest slot first: the client, the entry, what to do and (for a select) the stored path in `out` with its length.
    pub(crate) fn take_request(
        &mut self,
        out: &mut [u8; PATH],
    ) -> Option<(usize, usize, Req, usize)> {
        for c in 0..CLIENTS {
            for k in 0..ENTRIES {
                let s = &mut self.slots[c][k];
                if s.req == Req::None {
                    continue;
                }
                let req = s.req;
                s.req = Req::None;
                if req == Req::Select {
                    out[..s.plen].copy_from_slice(&s.path[..s.plen]);
                }
                return Some((c, k, req, s.plen));
            }
        }
        None
    }

    /// Publish what the state machine says: which clients are open, and the status (and the info of a READY entry) of each entry of an open client, except where a request is
    /// waiting. A client that is not open has no entries and no requests.
    pub(crate) fn publish(&mut self, open: &[bool; CLIENTS], snap: &[[Snap; ENTRIES]; CLIENTS]) {
        for c in 0..CLIENTS {
            self.open[c] = open[c];
            for k in 0..ENTRIES {
                let s = &mut self.slots[c][k];
                if !open[c] {
                    s.status = UNUSED;
                    s.req = Req::None;
                } else if s.req == Req::None {
                    s.status = snap[c][k].status;
                    s.info = snap[c][k].info;
                }
            }
        }
    }
}

impl PoolTable {
    /// Empty the cell (`start`), with `backed` said whether an adapter is installed, in sections of one slot each (about 190 bytes), never one of the whole 6 kB. The first
    /// section closes every client and drops `backed`, so while the rest runs a call that comes in (a task that preempts the start) finds no client and answers `E_NOT_FOUND`
    /// or `E_CONTRACT`, and nothing reads a slot that is half cleared; the last section sets `backed`. Zeroed in place: no copy through the stack.
    pub fn reset<C: CoreApi>(cell: &PoolCell, core: &C, backed: bool) {
        cell.with(core, |t| {
            t.backed = false;
            t.open = [false; CLIENTS];
        });
        for c in 0..CLIENTS {
            for k in 0..ENTRIES {
                // SAFETY: all-zero bytes are the empty slot (`SLOT0`): integers, `Req::None` is the first variant.
                cell.with(core, |t| unsafe {
                    (&mut t.slots[c][k] as *mut Slot).write_bytes(0, 1)
                });
            }
        }
        cell.with(core, |t| t.backed = backed);
    }
}

impl Default for PoolTable {
    fn default() -> Self {
        PoolTable::new()
    }
}

// ---- the exports: the body of each `dk_sample_pool_*` call that reads or changes the table ----------------------------------------------

/// `dk_sample_pool_add`: the index of the entry that holds the file (a new one, or the one that already holds the path), or a negative error. The result is final: the load
/// is queued, the entry reads `QUEUED` from now on.
///
/// # Safety
/// `path` is null or a NUL-terminated string.
pub unsafe fn pool_add<C: CoreApi>(cell: &PoolCell, core: &C, handle: u32, path: *const u8) -> i32 {
    if path.is_null() {
        return E_SEL_PATH;
    }
    let mut full = [0u8; PATH];
    let n = rooted_path(core, path, &mut full);
    let key = n.map_or(0, |n| path_key(&full[..n]));
    cell.with(core, |t| {
        if !t.backed {
            return E_CONTRACT;
        }
        let c = match t.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        match n {
            Ok(n) => t.add(c, key, &full[..n]),
            Err(e) => e,
        }
    })
}

/// `dk_sample_pool_set`: load the file into entry `index`; 0 or a negative error, final as `pool_add`.
///
/// # Safety
/// `path` is null or a NUL-terminated string.
pub unsafe fn pool_set<C: CoreApi>(
    cell: &PoolCell,
    core: &C,
    handle: u32,
    index: u32,
    path: *const u8,
) -> i32 {
    if path.is_null() {
        return E_SEL_PATH;
    }
    let mut full = [0u8; PATH];
    let n = rooted_path(core, path, &mut full);
    let key = n.map_or(0, |n| path_key(&full[..n]));
    cell.with(core, |t| {
        if !t.backed {
            return E_CONTRACT;
        }
        let c = match t.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        match n {
            Ok(n) => t.set(c, index, key, &full[..n]),
            Err(e) => e,
        }
    })
}

/// `dk_sample_pool_clear`: the entry is a hole from now on (`status` says UNUSED); the other indexes stay.
pub fn pool_clear<C: CoreApi>(cell: &PoolCell, core: &C, handle: u32, index: u32) -> i32 {
    cell.with(core, |t| match t.client(handle) {
        Ok(c) => t.clear(c, index),
        Err(e) => e,
    })
}

/// `dk_sample_pool_status`.
pub fn pool_status<C: CoreApi>(cell: &PoolCell, core: &C, handle: u32, index: u32) -> i32 {
    cell.with(core, |t| match t.client(handle) {
        Ok(c) => t.status(c, index),
        Err(e) => e,
    })
}

/// `dk_sample_pool_info`: the 16-byte `dk_sample_info` (big-endian words) of a READY entry.
///
/// # Safety
/// `out` is null or writable for 16 bytes.
pub unsafe fn pool_info<C: CoreApi>(
    cell: &PoolCell,
    core: &C,
    handle: u32,
    index: u32,
    out: *mut u8,
) -> i32 {
    let got = cell.with(core, |t| match t.client(handle) {
        Ok(c) => {
            if out.is_null() {
                Err(E_NOT_FOUND)
            } else {
                t.info(c, index)
            }
        }
        Err(e) => Err(e),
    });
    match got {
        Ok(i) => {
            let w = [i.frames, i.rate, i.channels, i.token];
            let mut k = 0;
            while k < 4 {
                core::ptr::copy_nonoverlapping(w[k].to_be_bytes().as_ptr(), out.add(k * 4), 4);
                k += 1;
            }
            0
        }
        Err(e) => e,
    }
}

/// `dk_sample_pool_path`: the entry's selection, root-relative, NUL-terminated, into `buf`; the length without the NUL.
///
/// # Safety
/// `buf` is null or writable for `cap` bytes.
pub unsafe fn pool_path<C: CoreApi>(
    cell: &PoolCell,
    core: &C,
    handle: u32,
    index: u32,
    buf: *mut u8,
    cap: u32,
) -> i32 {
    let mut stored = [0u8; PATH];
    let got = cell.with(core, |t| match t.client(handle) {
        Ok(c) => {
            if buf.is_null() {
                Err(E_SEL_PATH)
            } else {
                t.path(c, index, &mut stored)
            }
        }
        Err(e) => Err(e),
    });
    match got {
        Ok(n) => {
            let p = unrooted(root_of(core), &stored[..n]);
            if p.len() + 1 > cap as usize {
                return E_SEL_PATH;
            }
            core::ptr::copy_nonoverlapping(p.as_ptr(), buf, p.len());
            *buf.add(p.len()) = 0;
            p.len() as i32
        }
        Err(e) => e,
    }
}

#[cfg(test)]
mod tests;
