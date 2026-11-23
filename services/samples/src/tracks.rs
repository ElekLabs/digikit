//! Per-module stereo sample pool of 16 entries (docs/design/sample-api-1.5.md). The state machine is
//! `dkrt_sdk::sample_tracks` (device-neutral, host-tested); this file binds it to the runtime:
//! the imports `dk_sample_pool_add`, `_set`, `_clear`, `_status`, `_info` and `_path`, the backing adapter's calls (`pool.rs`, plug-in point `dkrt_pool.h`, version 1)
//! and the events adapters call (`dkrt_pool_done`, `dkrt_pool_lost`).
//! No firmware address and no `cf.*` anchor is used here. Without the backing adapter every
//! `sample_pool_add` returns E_CONTRACT.
//!
//! The track table of a module travels in its .dkm DSP section (version 5, record kind 2: the byte offsets of the 16 `dk_sample_pool_entry`
//! records and the 16 control sets in the module's DSP data). `dsp::claim` calls `reserve` with their L2 byte addresses and
//! `ready` (`DspPart::loaded`) when the DSP image is in; before that no DSP write is made.
//!
//! The table writes over the DSP link (close, drain, publish) are queued by [`crate::writes::Writes`].
//!
//! The pool list is not kept here: the module keeps it in its settings (the picker, sdk/src/sample/picker.c), and the settings are per project
//! while the `project` service is loaded.
//!
//! TODO (not wired yet): a request mailbox so API calls from the MIDI task cannot interleave with a tick step of the same track (an idle track is safe today).
use crate::files::{files_in_store, probe, root_of, rooted_path};
use crate::pool::Pool;
use crate::writes::Writes;
use core::mem::MaybeUninit;
use core::ptr::{addr_of_mut, write_volatile};
use dkrt_abi::core_api::{CoreApi, LOG_INFO};
use dkrt_sdk::sample_tracks::{
    store_inspect, unrooted, Core, Env, FileId, Handle, Inspect, Load, Note, Strack, PATH,
};
use dkrt_sdk::{logk, E_CONTRACT, E_NOT_FOUND, E_SEL_PATH};

pub(crate) const CLIENTS: usize = 2;
const ENTRIES: usize = 16;

/// The state machine of the tracks of all clients (`dkrt_sdk::sample_tracks`).
pub(crate) type TrackCore = Core<CLIENTS, ENTRIES>;

// The two tables a device test reads by symbol: the L2 byte addresses of the 16 records and 16 control sets of each client (D7 of docs/plans/rc3-briefs.md). `Tracks::reserve` is the
// only writer; the logic reads the copies in `Writes`.
#[no_mangle]
pub static mut DKRT_SMP_RECORDS: [u32; CLIENTS] = [0; CLIENTS];
#[no_mangle]
pub static mut DKRT_SMP_CONTROL: [u32; CLIENTS] = [0; CLIENTS];

/// The tracks of the clients: each module that has a track table in its DSP section owns one client slot.
///
/// All-zero bytes are the initial state after `start` of the core (`init`): `owner` holds the module record + 1 of each slot (0: free), so the owner object can make it
/// where it lives (a 35 kB state is never copied through the stack).
pub(crate) struct Tracks {
    core: TrackCore,
    /// Module record + 1 of the owner of each client slot (0: free; the port replaced the `usize::MAX` marker, and a record index is small, so `rec + 1` cannot wrap), and its local id.
    owner: [usize; CLIENTS],
    local: [u32; CLIENTS],
    writes: Writes,
    /// Pool losses since the start (wrapping).
    lost: u32,
}

impl Tracks {
    pub(crate) fn new() -> Tracks {
        let mut core = TrackCore::zeroed();
        core.start();
        Tracks {
            core,
            owner: [0; CLIENTS],
            local: [0; CLIENTS],
            writes: Writes::new(),
            lost: 0,
        }
    }

    /// Make the state in `slot`: zero bytes plus the few non-zero starting values of the core.
    pub(crate) fn init(slot: &mut MaybeUninit<Tracks>) {
        let p = slot.as_mut_ptr();
        // SAFETY: every field of `Tracks` is valid as zero bytes (`TrackCore::zeroed` is all-zero by its contract, the rest are integers, bools and arrays of them).
        unsafe {
            p.write_bytes(0, 1);
            (*addr_of_mut!((*p).core)).start();
        }
    }

    pub(crate) fn core(&self) -> &TrackCore {
        &self.core
    }

    /// The client slot of the calling module, if it has a track table.
    pub(crate) fn client(&self, handle: u32) -> Result<usize, i32> {
        let h = handle as usize;
        if h == 0 || h > CLIENTS || !self.core.is_open(h - 1) {
            return Err(E_NOT_FOUND);
        }
        Ok(h - 1)
    }

    /// Is a client slot free (checked by `dsp::admit` before it takes anything)?
    pub(crate) fn can_reserve(&self) -> bool {
        self.owner.contains(&0)
    }

    /// A module's track table was found in its DSP section: `records` and `control` are the L2 byte addresses of its 16 records and 16
    /// control sets. Nothing is written to the DSP before `ready`.
    pub(crate) fn reserve(
        &mut self,
        rec: usize,
        local_id: u32,
        records: u32,
        control: u32,
    ) -> bool {
        let mut i = 0;
        while i < CLIENTS {
            if self.owner[i] == 0 && self.core.open(i) {
                self.owner[i] = rec + 1;
                self.local[i] = local_id;
                self.writes.reserve(i, records, control);
                // SAFETY: the named tables are written only here (a device test reads them by symbol)
                unsafe {
                    write_volatile(addr_of_mut!(DKRT_SMP_RECORDS[i]), records);
                    write_volatile(addr_of_mut!(DKRT_SMP_CONTROL[i]), control);
                }
                return true;
            }
            i += 1;
        }
        false
    }

    /// The module's DSP image is in, so its table words exist: queued writes may go out.
    pub(crate) fn ready(&mut self, rec: usize) {
        let mut i = 0;
        while i < CLIENTS {
            if self.owner[i] == rec + 1 {
                self.writes.ready(i);
            }
            i += 1;
        }
    }

    /// A module's record went away (its machine rows are already cleared, so no voice runs): unbind its tracks without closing them in
    /// the DSP, release the pool references and free the client slot. A write already in flight ends by itself (its bytes are copied into the request, and
    /// the link is serialized, so a later load of the range lands after it). The mask has bit `i` set for each client slot that was freed (the caller resets what it keeps per client).
    pub(crate) fn release<C: CoreApi>(&mut self, core: &C, pool: &Pool, rec: usize) -> u32 {
        let mut freed = 0;
        let mut i = 0;
        while i < CLIENTS {
            if self.owner[i] == rec + 1 {
                self.writes.begin_teardown(i);
                let mut n = 0;
                while n < 4
                    && !self.core.teardown(
                        i,
                        &mut Device {
                            core,
                            pool,
                            writes: &mut self.writes,
                        },
                    )
                {
                    n += 1;
                }
                self.writes.end_teardown();
                self.writes.release(i);
                self.owner[i] = 0;
                freed |= 1 << i;
            }
            i += 1;
        }
        freed
    }

    /// Does module record `rec` hold a track table?
    pub(crate) fn has(&self, rec: usize) -> bool {
        self.owner.iter().any(|o| *o == rec + 1)
    }

    /// Is the table of module record `rec` live (its DSP image is in)?
    pub(crate) fn is_live(&self, rec: usize) -> bool {
        (0..CLIENTS).any(|i| self.owner[i] == rec + 1 && self.writes.is_live(i))
    }

    /// The handle of the calling module's table with local id `local_id`, or E_NOT_FOUND.
    pub(crate) fn tracks<C: CoreApi>(&self, core: &C, local_id: u32) -> i32 {
        let owner = core.current_source();
        if owner == 0 {
            return E_NOT_FOUND;
        }
        let rec = owner as usize - 1;
        let mut i = 0;
        while i < CLIENTS {
            if self.owner[i] == rec + 1 && self.local[i] == local_id {
                return i as i32 + 1;
            }
            i += 1;
        }
        E_NOT_FOUND
    }

    /// `dk_sample_pool_open`: the handle of the module's pool table with the lowest local id (the one table a module declares), as `tracks` gives it for
    /// that id; E_NOT_FOUND without a table.
    pub(crate) fn open_first<C: CoreApi>(&self, core: &C) -> i32 {
        let owner = core.current_source();
        if owner == 0 {
            return E_NOT_FOUND;
        }
        let rec = owner as usize - 1;
        let mut best = usize::MAX;
        let mut i = 0;
        while i < CLIENTS {
            if self.owner[i] == rec + 1 && (best == usize::MAX || self.local[i] < self.local[best])
            {
                best = i;
            }
            i += 1;
        }
        if best == usize::MAX {
            return E_NOT_FOUND;
        }
        self.tracks(core, self.local[best])
    }

    /// `sample_pool_add`: the pool index of the file (a new entry, or the one that already holds the path), or a negative error.
    ///
    /// # Safety
    /// `path` is null or a NUL-terminated string.
    pub(crate) unsafe fn pool_add<C: CoreApi>(
        &mut self,
        core: &C,
        pool: &Pool,
        handle: u32,
        path: *const u8,
    ) -> i32 {
        if path.is_null() {
            return E_SEL_PATH;
        }
        if !pool.present() {
            return E_CONTRACT;
        }
        let c = match self.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let mut full = [0u8; PATH];
        let n = match rooted_path(core, path, &mut full) {
            Ok(n) => n,
            Err(e) => return e,
        };
        let sr = core.irq_mask();
        let r = self.core.pool_add(c, &full[..n]);
        core.irq_restore(sr);
        r
    }

    /// `sample_pool_set`: load the file into entry `index` (replacing what was there); 0 or a negative error.
    ///
    /// # Safety
    /// `path` is null or a NUL-terminated string.
    pub(crate) unsafe fn pool_set<C: CoreApi>(
        &mut self,
        core: &C,
        pool: &Pool,
        handle: u32,
        index: u32,
        path: *const u8,
    ) -> i32 {
        if path.is_null() {
            return E_SEL_PATH;
        }
        if !pool.present() {
            return E_CONTRACT;
        }
        let c = match self.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        let mut full = [0u8; PATH];
        let n = match rooted_path(core, path, &mut full) {
            Ok(n) => n,
            Err(e) => return e,
        };
        let sr = core.irq_mask();
        let r = self.core.pool_set(c, index as usize, &full[..n]);
        core.irq_restore(sr);
        r
    }

    /// `sample_pool_clear`: the entry becomes UNUSED (a hole); the other indexes stay.
    pub(crate) fn pool_clear<C: CoreApi>(&mut self, core: &C, handle: u32, track: u32) -> i32 {
        match self.client(handle) {
            Ok(c) => {
                let sr = core.irq_mask();
                let r = self.core.unbind(c, track as usize);
                core.irq_restore(sr);
                r
            }
            Err(e) => e,
        }
    }

    pub(crate) fn status(&self, handle: u32, track: u32) -> i32 {
        match self.client(handle) {
            Ok(c) => self.core.status(c, track as usize),
            Err(e) => e,
        }
    }

    /// `sample_pool_path`: the entry's current selection, root-relative, NUL-terminated, into `buf`; the length without the NUL.
    ///
    /// # Safety
    /// `buf` is null or writable for `cap` bytes.
    pub(crate) unsafe fn path<C: CoreApi>(
        &self,
        core: &C,
        handle: u32,
        track: u32,
        buf: *mut u8,
        cap: u32,
    ) -> i32 {
        let c = match self.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        if buf.is_null() {
            return E_SEL_PATH;
        }
        match self.core.path(c, track as usize) {
            Ok(stored) => {
                let p = unrooted(root_of(core), stored);
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

    /// Copies a 16-byte `dk_sample_info` (big-endian words, as the module sees them) when the track is READY.
    ///
    /// # Safety
    /// `out` is null or writable for 16 bytes.
    pub(crate) unsafe fn info(&self, handle: u32, track: u32, out: *mut u8) -> i32 {
        let c = match self.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        if out.is_null() {
            return E_NOT_FOUND;
        }
        match self.core.info(c, track as usize) {
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

    /// Bounded overview copy; no file or PCM access in the UI task.
    ///
    /// # Safety
    /// `out` is null or writable for `capacity` bytes.
    pub(crate) unsafe fn peaks(
        &self,
        pool: &Pool,
        handle: u32,
        track: u32,
        token: u32,
        out: *mut u8,
        capacity: u32,
    ) -> i32 {
        let c = match self.client(handle) {
            Ok(c) => c,
            Err(e) => return e,
        };
        if out.is_null() || (out as usize & 1) != 0 || capacity < 2048 || !pool.peaks_present() {
            return E_CONTRACT;
        }
        match self.core.peak_handle(c, track as usize, token) {
            Ok(h) => pool.peaks(&h, out, capacity),
            Err(e) => e,
        }
    }

    /// One step of every track, in the UI loop. Without a backing adapter nothing runs.
    pub(crate) fn tick<C: CoreApi>(&mut self, core: &C, pool: &Pool) {
        if !pool.present() {
            return;
        }
        // Optional on its own: the adapter delivers completions and finds a stock reset here, before the runtime steps the tracks.
        // SAFETY: the adapter's call (`dkpool_tick`). It may call `pool_done` and `pool_lost` of this owner through the file's cell while this `&mut self` is live: an alias of the exclusive borrow. It is sound in practice only because nothing of the state is read or written before the call and every later access sees the state the callbacks left; keep this call first.
        unsafe { pool.tick() };
        // SAFETY: the core's link calls
        unsafe { self.writes.pump(core) };
        self.core.tick(&mut Device {
            core,
            pool,
            writes: &mut self.writes,
        });
        // SAFETY: as above
        unsafe { self.writes.pump(core) };
    }

    /// Adapter to runtime: load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT). The slot `pool_done` of the service file calls it.
    pub(crate) fn pool_done(&mut self, req: u32, result: i32) {
        self.core.done(req, result);
    }

    /// Adapter to runtime: the backing was lost, all handles are invalid. The slot `pool_lost` of the service file calls it.
    pub(crate) fn pool_lost<C: CoreApi>(&mut self, core: &C, pool: &Pool) {
        self.lost = self.lost.wrapping_add(1);
        // SAFETY: the core's log call
        unsafe {
            core.log(
                logk::POOL_LOST,
                logk::L_WARN << logk::LEVEL_SHIFT,
                [self.lost, 0, 0, 0, 0],
            )
        };
        self.core.lost(&mut Device {
            core,
            pool,
            writes: &mut self.writes,
        });
    }
}

/// The environment the track state machine runs in: the pool adapter's `dkpool_*` calls, the table writes, and the core for the platform facts and the log.
struct Device<'a, C: CoreApi> {
    core: &'a C,
    pool: &'a Pool,
    writes: &'a mut Writes,
}

impl<C: CoreApi> Env for Device<'_, C> {
    fn inspect(&mut self, path: &[u8]) -> Result<Inspect, i32> {
        let mut head = [0u8; 64];
        // SAFETY: the core's storage calls
        let id = unsafe { probe(self.core, path, Some(&mut head))? };
        // Size and format only: no PCM is read here (it can be megabytes), the pool adapter reads it.
        // This branch is a format choice (the store identity and header check, or the native header alone), not a storage route: `probe` reads both the same way.
        if files_in_store(self.core) {
            return store_inspect(id.size, &id.hash, &head);
        }
        let (frames, channels) = dkrt_sdk::sample::native_header(&head, id.size)?;
        Ok(Inspect {
            id,
            frames,
            rate: dkrt_sdk::sample::RATE,
            channels,
        })
    }
    fn stat(&mut self, path: &[u8]) -> Result<FileId, i32> {
        // SAFETY: as `inspect`
        unsafe { probe(self.core, path, None) }
    }
    fn load(&mut self, req: u32, id: &FileId, path: &[u8], out: &mut Handle) -> Load {
        // The adapter gets a NUL-terminated copy: the runtime's path buffers are not.
        let mut z = [0u8; PATH + 1];
        z[..path.len()].copy_from_slice(path);
        match unsafe { self.pool.load(req, id, z.as_ptr(), out) } {
            0 => Load::Done,
            1 => Load::Queued,
            e => Load::Err(e),
        }
    }
    fn release(&mut self, h: &Handle) {
        unsafe { self.pool.release(h) }
    }
    fn cancel(&mut self, req: u32) {
        unsafe { self.pool.cancel(req) }
    }
    fn check(&mut self, h: &Handle) -> i32 {
        unsafe { self.pool.check(h) }
    }
    fn free_frames(&mut self) -> u32 {
        unsafe { self.pool.free_frames() }
    }
    fn close(&mut self, client: usize, track: usize) {
        self.writes.close(client, track);
    }
    fn drained(&mut self, client: usize, track: usize) -> bool {
        self.writes.drained(client, track)
    }
    fn published(&mut self, client: usize, track: usize) -> i32 {
        self.writes.published(client, track)
    }
    fn note(&mut self, n: Note) {
        match n {
            Note::Loaded {
                client,
                entry,
                frames,
                base_l,
                token,
            } => unsafe {
                self.core.log(
                    logk::POOL_LOAD,
                    LOG_INFO,
                    [client << 8 | entry, 0, frames, base_l, token],
                )
            },
            Note::Invalid {
                client,
                entry,
                reason,
                token,
                count,
            } => unsafe {
                self.core.log(
                    logk::POOL_INVALID,
                    LOG_INFO,
                    [client << 8 | entry, reason, token, count, 0],
                )
            },
        }
    }
    fn publish(&mut self, client: usize, track: usize, rec: &Strack) {
        // SAFETY: the core's log call
        unsafe {
            self.core.log(
                logk::POOL_PUBLISH,
                LOG_INFO,
                [
                    (client as u32) << 8 | track as u32,
                    rec.token,
                    rec.frames,
                    rec.base_l,
                    rec.base_r,
                ],
            )
        };
        self.writes.publish(client, track, rec);
    }
}

#[cfg(test)]
mod tests;
