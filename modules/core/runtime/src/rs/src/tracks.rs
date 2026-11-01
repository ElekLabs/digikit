//! Per-track stereo samples (API 1.5, docs/design/sample-api-1.5.md). The state machine is
//! `dkrt_sdk::sample_tracks` (device-neutral, host-tested); this file binds it to the runtime:
//! the Api entries, the optional `dkpool_*` backing symbols (plug-in point `dkrt_pool.h`, version 1)
//! and the events adapters call (`dkrt_pool_done`, `dkrt_pool_lost`, `dkrt_project_changed`).
//! No firmware address and no `cf.*` anchor is used here. Without the backing adapter every
//! `sample_select` returns E_CONTRACT.
//!
//! TODO (not wired yet, so no module can declare a track table today and `sample_tracks` returns
//! E_CONTRACT): the DK_DESC_SAMPLE_TRACKS lowering into the .dkm DSP section and `reserve` here;
//! the DSP-side `Env` operations (close and drain-ack words, record publish over the DSP link);
//! the header inspection of stereo native files (`inspect`/`stat`); persistence of the selections
//! to the digikit store (they live in memory per project now).
use crate::sample;
use core::hint::black_box;
use core::ptr::addr_of_mut;
use dkrt_sdk::sample_tracks::{Core, Env, FileId, Handle, Inspect, Load, Strack, TRACKS};
use dkrt_sdk::{E_CONTRACT, E_NOT_FOUND, E_SEL_PATH};

pub const DKPOOL_VERSION: u32 = 1;
const CLIENTS: usize = 2;
const ENTRIES: usize = 16;
const PROJECTS: usize = 4;

extern "C" {
    // Optional symbols of the sample-pool adapter (`[object] optional` in module.toml): 0 when it is not in the image.
    fn dkpool_version() -> u32;
    fn dkpool_load(req: u32, id: *const FileId, path: *const u8, out: *mut Handle) -> i32;
    fn dkpool_release(h: *const Handle);
    fn dkpool_check(h: *const Handle) -> i32;
    fn dkpool_free_frames() -> u32;
}

static mut CORE: Core<CLIENTS, ENTRIES, PROJECTS> = Core::new();
/// Module record + 1 of the owner of each client slot (0 = free) and its DSP table addresses.
static mut OWNER: [usize; CLIENTS] = [usize::MAX; CLIENTS];
static mut LOCAL: [u32; CLIENTS] = [0; CLIENTS];
/// 0 not checked, 1 usable, 2 absent or wrong version.
static mut POOL: u8 = 0;

unsafe fn core_() -> &'static mut Core<CLIENTS, ENTRIES, PROJECTS> {
    &mut *addr_of_mut!(CORE)
}

/// Is a backing adapter of the right version in the image?
unsafe fn pool_ok() -> bool {
    if POOL == 0 {
        let present = black_box(dkpool_version as *const () as usize) != 0
            && black_box(dkpool_load as *const () as usize) != 0
            && black_box(dkpool_release as *const () as usize) != 0
            && black_box(dkpool_check as *const () as usize) != 0
            && black_box(dkpool_free_frames as *const () as usize) != 0;
        POOL = if present && dkpool_version() == DKPOOL_VERSION { 1 } else { 2 };
    }
    POOL == 1
}

struct Device;
impl Env for Device {
    // TODO: header check and stat of stereo native files (needs the listing row identity).
    fn inspect(&mut self, _path: &[u8]) -> Result<Inspect, i32> {
        Err(E_CONTRACT)
    }
    fn stat(&mut self, _path: &[u8]) -> Result<FileId, i32> {
        Err(E_CONTRACT)
    }
    fn load(&mut self, req: u32, id: &FileId, path: &[u8], out: &mut Handle) -> Load {
        // The adapter gets a NUL-terminated copy: the runtime's path buffers are not.
        let mut z = [0u8; dkrt_sdk::sample_tracks::PATH + 1];
        z[..path.len()].copy_from_slice(path);
        match unsafe { dkpool_load(req, id, z.as_ptr(), out) } {
            0 => Load::Done,
            1 => Load::Queued,
            e => Load::Err(e),
        }
    }
    fn release(&mut self, h: &Handle) {
        unsafe { dkpool_release(h) }
    }
    fn check(&mut self, h: &Handle) -> i32 {
        unsafe { dkpool_check(h) }
    }
    fn free_frames(&mut self) -> u32 {
        unsafe { dkpool_free_frames() }
    }
    // TODO: DSP words of the track (CLOSED, COUNT) and the record publish over the DSP link.
    fn close(&mut self, _client: usize, _track: usize) {}
    fn drained(&mut self, _client: usize, _track: usize) -> bool {
        false
    }
    fn publish(&mut self, _client: usize, _track: usize, _rec: &Strack) {}
}

/// The client slot of the calling module, if it has a track table.
unsafe fn client(handle: u32) -> Result<usize, i32> {
    let h = handle as usize;
    if h == 0 || h > CLIENTS || !core_().is_open(h - 1) {
        return Err(E_NOT_FOUND);
    }
    Ok(h - 1)
}

/// A module's track table was reserved by the loader (the DSP section lowering is a TODO, so nothing calls this yet).
#[allow(dead_code)]
pub unsafe fn reserve(rec: usize, local_id: u32) -> bool {
    let mut i = 0;
    while i < CLIENTS {
        if OWNER[i] == usize::MAX && core_().open(i) {
            OWNER[i] = rec;
            LOCAL[i] = local_id;
            return true;
        }
        i += 1;
    }
    false
}

/// A module's record went away: unbind its tracks. True once nothing can touch its memory.
#[allow(dead_code)]
pub unsafe fn remove(rec: usize) -> bool {
    let mut done = true;
    let mut i = 0;
    while i < CLIENTS {
        if OWNER[i] == rec {
            if core_().teardown(i, &mut Device) {
                OWNER[i] = usize::MAX;
            } else {
                done = false;
            }
        }
        i += 1;
    }
    done
}

/// Does module record `rec` hold a track table (the bank API refuses it then)?
pub unsafe fn has(rec: usize) -> bool {
    let mut i = 0;
    while i < CLIENTS {
        if OWNER[i] == rec {
            return true;
        }
        i += 1;
    }
    false
}

pub unsafe fn tracks(local_id: u32) -> i32 {
    let owner = crate::dlog::current_source();
    if owner == 0 {
        return E_NOT_FOUND;
    }
    let rec = owner as usize - 1;
    if sample::has_bank(rec) {
        return E_CONTRACT;
    }
    let mut i = 0;
    while i < CLIENTS {
        if OWNER[i] == rec && LOCAL[i] == local_id {
            return i as i32 + 1;
        }
        i += 1;
    }
    E_NOT_FOUND
}

pub unsafe fn select(handle: u32, track: u32, path: *const u8) -> i32 {
    if path.is_null() {
        return E_SEL_PATH;
    }
    if track as usize >= TRACKS || !pool_ok() {
        return E_CONTRACT;
    }
    let c = match client(handle) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let mut copy = [0u8; dkrt_sdk::sample_tracks::PATH];
    let mut n = 0;
    while n < copy.len() {
        let v = *path.add(n);
        if v == 0 {
            break;
        }
        copy[n] = v;
        n += 1;
    }
    if n == copy.len() {
        return E_SEL_PATH;
    }
    core_().select(c, track as usize, &copy[..n])
}

pub unsafe fn unbind(handle: u32, track: u32) -> i32 {
    match client(handle) {
        Ok(c) => core_().unbind(c, track as usize),
        Err(e) => e,
    }
}

pub unsafe fn status(handle: u32, track: u32) -> i32 {
    match client(handle) {
        Ok(c) => core_().status(c, track as usize),
        Err(e) => e,
    }
}

/// Copies a 16-byte `dk_sample_info` (big-endian words, as the module sees them) when the track is READY.
pub unsafe fn info(handle: u32, track: u32, out: *mut u8) -> i32 {
    let c = match client(handle) {
        Ok(c) => c,
        Err(e) => return e,
    };
    if out.is_null() {
        return E_NOT_FOUND;
    }
    match core_().info(c, track as usize) {
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

/// One step of every track, in the UI loop. Without a backing adapter nothing runs.
pub unsafe fn tick() {
    if POOL == 2 || !pool_ok() {
        return;
    }
    core_().tick(&mut Device);
}

/// Adapter to runtime: load `req` finished (`result` 0, E_POOL, E_READ or E_FORMAT).
#[no_mangle]
pub unsafe extern "C" fn dkrt_pool_done(req: u32, result: i32) {
    core_().done(req, result);
}
/// Adapter to runtime: the backing was lost, all handles are invalid.
#[no_mangle]
pub unsafe extern "C" fn dkrt_pool_lost() {
    core_().lost();
}
/// Project adapter to runtime: the current project slot changed.
#[no_mangle]
pub unsafe extern "C" fn dkrt_project_changed(slot: u32) {
    core_().project_changed(slot);
}
