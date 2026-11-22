//! Live changes of the module set (docs/plans/extension-model.md E5 and E5c): what the runtime holds once for every kind of module. This file has the refusal order and
//! the job: one job with three operations (load, unload, reload), made of the loader steps and the DSP wait. Per-kind work sits behind the `Service` trait, never in here.

use dkrt_abi::platform::{blocks_since, Platform};
use dkrt_sdk::*;

/// DSP blocks the reload waits after it closed every way into the module's code (its tick, its commands, its sinks): the tasks that were inside have
/// returned and a DSP block that read its rows has ended. The block clock runs about 1500 times a second.
pub const DRAIN_BLOCKS: u32 = 2;

/// Have `DRAIN_BLOCKS` blocks passed since `start` (the block count when the entries were closed)? A platform with no block clock (a device without a DSP,
/// the host tests) has nothing to wait for: always true.
pub fn drained<P: Platform>(p: &P, start: Option<u32>) -> bool {
    blocks_since(p, start).is_none_or(|n| n >= DRAIN_BLOCKS)
}

/// While a reload runs: the owner of the module being reloaded (module record index + 1), else 0. From the checks of the new file until the end of the job
/// (`CoreApi::live_op`). The reload job sets and clears it; a service reads it where the module's own rows must not count against the new file (the
/// checks of its types and rows) and where its teardown differs from a plain unload (the placeholder rows). A load and an unload leave it 0: a plain unload
/// writes gap rows, and a load has no rows of its own to leave out.
static mut PHASE: u32 = 0;

/// The reload phase now.
pub fn phase() -> u32 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PHASE)) }
}

/// Start the reload phase of module `owner` (record index + 1), or end it (0). Only the reload job calls it.
pub(crate) fn set_phase(owner: u8) {
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PHASE), owner as u32) }
}

/// A live job changed the module set: the counter that tells a host and the menus their lists of modules and machines are out of date (`DkrtBootInfo.reloads`).
pub(crate) unsafe fn note_reloaded() {
    let b = crate::boot::info();
    b.reloads = b.reloads.wrapping_add(1);
}

/// What the caller knows about the system when a reload is asked for. Each field is read where it lives (the loader, the card, the module's
/// records); `refusal` only puts the answers in order.
#[derive(Clone, Copy, Default)]
pub struct Facts {
    /// This boot is in safe mode.
    pub safe_mode: bool,
    /// An upload is open, or a store write is in flight (`E_STATE`): the files of the store are not settled.
    pub upload_open: bool,
    /// A card transfer or a store job is in flight.
    pub card_busy: bool,
    /// A menu object was built for the module (`dkrt_menu_built`: it called `dk_ui_menu_new`, or the DIGIKIT menu holds it while its SETTINGS page is open until `dkrt_menu_release`; the DIGIKIT menu's own menus count for no content module): the menu keeps
    /// pointers into the module's code and data.
    pub menus: bool,
}

/// The refusal of a live job (a load, an unload or a reload), or `None` when it may go on. One function, one order: safe mode, the firmware's transport (playing, paused, or not
/// readable), an open upload, card operations, menus of the module (`Facts::menus`: false for a load, which has no module yet). The first that holds names the status.
/// The services' own refusals (`Service::teardown_refusal`: a service that cannot give back the module's state now) come after it, for an unload and a reload: `refusal_with_services`.
pub unsafe fn refusal<P: Platform>(p: &P, f: &Facts) -> Option<i32> {
    if f.safe_mode {
        return Some(E_SAFE_MODE);
    }
    match p.play_state() {
        Some(0) => {}
        _ => return Some(E_RELOAD_PLAYING),
    }
    if f.upload_open {
        return Some(dkrt_sdk::proto::E_STATE);
    }
    if f.card_busy {
        return Some(E_CARD_BUSY);
    }
    if f.menus {
        return Some(E_RELOAD_MENUS);
    }
    None
}

/// `refusal`, then the refusals of the services about module record `owner - 1` (`Service::teardown_refusal`, every service in table order). The reload job calls this.
#[cfg_attr(not(test), allow(dead_code))]
pub unsafe fn refusal_with_services<P: Platform>(p: &P, f: &Facts, services: &dkrt_abi::service::ServiceTable, owner: u8) -> Option<i32> {
    if let Some(r) = refusal(p, f) {
        return Some(r);
    }
    let r = services.teardown_refusal(owner);
    if r != OK {
        return Some(r);
    }
    None
}


// ---- the job ----------------------------------------------------------------------------------------------------------------------------------

/// What the live job asks of the system, one call per step. `op` is `LIVE_LOAD`, `LIVE_UNLOAD` or `LIVE_RELOAD`; a module of a load has no record yet (`None`). The real one (`RealOps`) calls the loader, the services and the boot record; a host test scripts it.
pub trait Ops {
    /// The module record and selection index of the loaded content module that came from `path`.
    unsafe fn find(&mut self, path: *const u8) -> Option<(usize, usize)>;
    /// Is `path` the file of a loaded service (a provider record)? A reload is for content modules only.
    unsafe fn is_service(&mut self, path: *const u8) -> bool;
    /// The selection index a load of the file at `path` takes (`boot::live_index`), or `E_SEL_COUNT`.
    unsafe fn alloc_index(&mut self, path: *const u8) -> Result<usize, i32>;
    /// The refusals before anything changes: `reload::refusal` over the facts of the system (safe mode, an open upload or a card transfer), and for the module `rec` (an
    /// unload or a reload) its menus and `Service::teardown_refusal`.
    unsafe fn refusal(&mut self, rec: Option<usize>) -> Option<i32>;
    /// The checks of the file before anything changes: `Loader::reload_check` of a reload, `Loader::live_check_load` of a load at index `k`. An unload has no file to check.
    unsafe fn check(&mut self, op: u32, rec: usize, k: usize, path: *const u8) -> i32;
    /// The reload phase (`CoreApi::live_op`) of the module with this owner (record index + 1), or off (0).
    fn phase(&mut self, owner: u8);
    /// `Loader::reload_quiesce`.
    unsafe fn quiesce(&mut self, rec: usize) -> i32;
    /// `Loader::reload_abort`: the old module gets its tick and commands back; false when its `fini` ran.
    unsafe fn abort(&mut self, rec: usize) -> bool;
    /// Was a menu object built for module record `rec` since the refusal was read (`dkrt_menu_built`)? The drain lasts many UI passes, and a menu built meanwhile points into the
    /// module's code, so the job asks again right before the teardown.
    unsafe fn menu_built(&mut self, rec: usize) -> bool;
    /// `Loader::unload_rec`.
    unsafe fn unload(&mut self, rec: usize) -> i32;
    /// Does the DSP loader hold a module it could not release? Permanent until the next boot.
    unsafe fn dsp_retained(&mut self) -> bool;
    /// How far the DSP loader has got with its loads (a counter that moves with every image or row it sends and every load it starts).
    unsafe fn dsp_progress(&mut self) -> u32;
    /// The boot record's view of the module goes to INIT (`boot::begin_live`).
    unsafe fn begin_load(&mut self, k: usize);
    /// `Loader::reload_load` of a reload (into record `rec`), `Loader::live_load` of a load (into a free record at index `k`), and the boot record's account of the result.
    unsafe fn load(&mut self, op: u32, rec: usize, k: usize, path: *const u8) -> i32;
    /// The end state of an unload: the index is free again (`Loader::retire_index`) and the boot record says UNLOADED (`boot::unloaded`).
    unsafe fn unloaded(&mut self, k: usize);
    /// The end state of a load that failed: the index is free again and its boot record row is as before the load (`boot::restore_live`).
    unsafe fn retire(&mut self, k: usize);
    /// The DSP part of the new file: `None` while it goes on, else its result (the boot record's `results[k]`).
    unsafe fn dsp_result(&mut self, k: usize) -> Option<i32>;
    /// Give up the DSP part (`Loader::cancel_dsp`): the transfer is taken down as a failed one is, and the module is torn down.
    unsafe fn cancel_dsp(&mut self, rec: usize, code: i32) -> bool;
    /// Every service: `Service::on_loaded` of module record `rec` (a module with no DSP gets it here; one with DSP got it from the DSP loader).
    unsafe fn on_loaded(&mut self, rec: usize);
    /// The end of a job that got past the checks, whatever the result: `Service::live_done` on every service (placeholders freed, the machine rows of a module that is not
    /// back written as gap rows, the machine list rebuilt), and the counter when the module was loaded or torn down (`changed`). A refusal before the checks skips it:
    /// nothing was touched.
    unsafe fn finish(&mut self, ok: bool, changed: bool);
    /// The block count, `None` without a block clock.
    fn blocks(&mut self) -> Option<u32>;
}

/// Where the job is (`RELOAD_STATE` step byte).
pub mod step {
    pub const IDLE: u8 = 0;
    pub const CHECK: u8 = 1;
    pub const QUIESCE: u8 = 2;
    pub const UNLOAD: u8 = 3;
    pub const LOAD: u8 = 4;
    pub const DSP: u8 = 5;
    pub const DONE: u8 = 6;
}

/// Blocks (1500 a second) a wait may take before the job gives up: the drain, the teardown and the DSP part each.
const WAIT_BLOCKS: u32 = 6000;
/// UI passes for the same, counted whatever the block clock says: a stalled or missing clock still ends the wait (the UI loop runs 30 to 120 passes a second).
const WAIT_PASSES: u32 = 1800;

/// The record of a load before its file is in one (and after a load that failed).
const NO_REC: usize = usize::MAX;

/// One live job at a time. `poll` runs one step per UI pass and never waits.
///
/// The steps of each operation: load CHECK, LOAD, DSP (a module with a DSP part), DONE; unload CHECK, QUIESCE, UNLOAD, DONE; reload CHECK, QUIESCE, UNLOAD, LOAD, DSP, DONE.
pub struct Job {
    pub step: u8,
    /// 0 none yet, 1 queued or running, 2 finished.
    pub phase: u8,
    /// The load status of the new file, or the refusal.
    pub result: i32,
    /// The old module was torn down (its `fini` ran): the module is not loaded unless the new file loaded. A refusal or an abort before that leaves the old module running.
    /// Always false for a load.
    pub torn: bool,
    /// `LIVE_LOAD`, `LIVE_UNLOAD` or `LIVE_RELOAD` of the last job (0: none yet).
    pub op: u8,
    path: [u8; 64],
    rec: usize,
    sel: usize,
    start: Option<u32>,
    passes: u32,
    /// `Ops::dsp_progress` at the last pass of the DSP wait.
    progress: u32,
}

/// What `poll` reads about the operation: a load has no old module, an unload no new file.
fn is_load(op: u8) -> bool {
    op as u32 == LIVE_LOAD
}
fn is_unload(op: u8) -> bool {
    op as u32 == LIVE_UNLOAD
}
fn is_reload(op: u8) -> bool {
    op as u32 == LIVE_RELOAD
}

impl Job {
    pub const fn new() -> Job {
        Job { step: step::IDLE, phase: 0, result: OK, torn: false, op: 0, path: [0; 64], rec: NO_REC, sel: 0, start: None, passes: 0, progress: 0 }
    }

    /// Queue job `op` for the module at `path` (NUL terminated, at most 63 bytes): a load of a file that no loaded module came from, or an unload or a reload of the module
    /// loaded from that path. `OK`; `E_BUSY` while one is queued or running; `E_MSG` for a path that does not fit or an unknown `op`; `E_RELOAD_RETAINED` while the DSP loader holds
    /// a module it could not release (restart the device); `E_FORMAT` for the file of a service; `E_NOT_FOUND` when an unload or a reload names a path no loaded content module
    /// came from; `E_LOADED` when a load names the path of a loaded module.
    pub unsafe fn queue<O: Ops>(&mut self, o: &mut O, op: u32, path: *const u8) -> i32 {
        if self.phase == 1 {
            return E_BUSY;
        }
        if op != LIVE_LOAD && op != LIVE_UNLOAD && op != LIVE_RELOAD {
            return dkrt_sdk::proto::E_MSG;
        }
        let n = crate::cstr_len(path, 64);
        if n == 0 || n >= 64 {
            return dkrt_sdk::proto::E_MSG;
        }
        if o.dsp_retained() {
            return E_RELOAD_RETAINED;
        }
        if o.is_service(path) {
            return E_FORMAT;
        }
        let found = o.find(path).is_some();
        if op == LIVE_LOAD && found {
            return E_LOADED;
        }
        if op != LIVE_LOAD && !found {
            return E_NOT_FOUND;
        }
        let mut i = 0;
        while i < 64 {
            self.path[i] = if i < n { *path.add(i) } else { 0 };
            i += 1;
        }
        self.phase = 1;
        self.op = op as u8;
        self.step = step::CHECK;
        self.result = OK;
        self.torn = false;
        self.rec = NO_REC;
        self.passes = 0;
        self.start = None;
        OK
    }

    /// One more pass of a wait: true when it has lasted too long, by the block count or, whatever that does, by the passes.
    fn timed_out<O: Ops>(&mut self, o: &mut O) -> bool {
        self.passes += 1;
        let blocks = match (self.start, o.blocks()) {
            (Some(a), Some(b)) => b.wrapping_sub(a) > WAIT_BLOCKS,
            _ => false,
        };
        blocks || self.passes > WAIT_PASSES
    }

    fn wait_restart<O: Ops>(&mut self, o: &mut O) {
        self.start = o.blocks();
        self.passes = 0;
    }

    /// The job ended before anything changed: a refusal or a failed check.
    fn refused(&mut self, result: i32) {
        self.result = result;
        self.step = step::DONE;
        self.phase = 2;
    }

    /// The job ends with `result` after its checks: a load that did not come up frees its index, the services hear the end (`finish`), the phase is off.
    unsafe fn end<O: Ops>(&mut self, o: &mut O, result: i32, ok: bool) {
        if is_load(self.op) && !ok {
            o.retire(self.sel);
        }
        o.finish(ok, self.torn);
        o.phase(0);
        self.result = result;
        self.step = step::DONE;
        self.phase = 2;
    }

    /// The first step: nothing changes before the refusals and the checks pass, and a refusal ends the job with everything as it was. The path is looked up again (the module may
    /// be gone, or have come, since the queue).
    #[inline(never)]
    unsafe fn check<O: Ops>(&mut self, o: &mut O) {
        let path = self.path.as_ptr();
        let found = o.find(path);
        let mut rec = None;
        if is_load(self.op) {
            if found.is_some() {
                return self.refused(E_LOADED);
            }
            match o.alloc_index(path) {
                Ok(k) => self.sel = k,
                Err(e) => return self.refused(e),
            }
        } else {
            match found {
                Some((r, k)) => {
                    self.rec = r;
                    self.sel = k;
                    rec = Some(r);
                }
                None => return self.refused(E_NOT_FOUND),
            }
            // the phase is on for the checks of a reload too: the services leave the module's own rows out of "used by another module" (types, rows)
            if is_reload(self.op) {
                o.phase(self.rec as u8 + 1);
            }
        }
        if o.dsp_retained() {
            o.phase(0);
            return self.refused(E_RELOAD_RETAINED);
        }
        if let Some(r) = o.refusal(rec) {
            o.phase(0);
            return self.refused(r);
        }
        if !is_unload(self.op) {
            let r = o.check(self.op as u32, self.rec, self.sel, path);
            if r != OK {
                o.phase(0);
                return self.refused(r);
            }
        }
        if is_load(self.op) {
            o.begin_load(self.sel);
            self.step = step::LOAD;
        } else {
            self.wait_restart(o);
            self.step = step::QUIESCE;
        }
    }

    /// One step. Called once per UI pass (`dkrt_tick`).
    pub unsafe fn poll<O: Ops>(&mut self, o: &mut O) {
        if self.phase != 1 {
            return;
        }
        let path = self.path.as_ptr();
        if self.step == step::CHECK {
            self.check(o);
        } else if self.step == step::QUIESCE {
            let r = o.quiesce(self.rec);
            if r == OK {
                self.wait_restart(o);
                self.step = step::UNLOAD;
            } else if r != E_BUSY || self.timed_out(o) {
                // the drain did not end: nothing was freed, the old module gets its tick and commands back
                o.abort(self.rec);
                self.end(o, r, false);
            }
        } else if self.step == step::UNLOAD {
            if !self.torn && o.menu_built(self.rec) {
                // a page of the module's menus was built during the drain: nothing was freed, the old module gets its tick and commands back
                o.abort(self.rec);
                self.end(o, E_RELOAD_MENUS, false);
                return;
            }
            let r = o.unload(self.rec);
            if r == OK {
                self.torn = true;
                if is_unload(self.op) {
                    o.unloaded(self.sel);
                    self.end(o, OK, true);
                } else {
                    o.begin_load(self.sel);
                    self.step = step::LOAD;
                }
            } else if r != E_BUSY || o.dsp_retained() || self.timed_out(o) {
                // `unload_rec` answers E_BUSY only before `fini` (a file or I/O step of the module still runs, or the DSP rows cannot be cleared), so the old module is whole and gets
                // its tick and commands back. A DSP loader that holds rows it could not clear is permanent: say so at once instead of waiting out the budget.
                o.abort(self.rec);
                let retained = o.dsp_retained();
                self.end(o, if retained { E_RELOAD_RETAINED } else { r }, false);
            }
        } else if self.step == step::LOAD {
            let r = o.load(self.op as u32, self.rec, self.sel, path);
            if is_load(self.op) {
                // the loader chose the record
                self.rec = o.find(path).map_or(NO_REC, |f| f.0);
            }
            if r == OK {
                o.on_loaded(self.rec);
                self.end(o, OK, true);
            } else if r == LOADING {
                self.wait_restart(o);
                self.progress = o.dsp_progress();
                self.step = step::DSP;
            } else {
                self.end(o, r, false);
            }
        } else if self.step == step::DSP {
            match o.dsp_result(self.sel) {
                Some(r) => self.end(o, r, r == OK),
                None => {
                    // one budget for a stalled load, not for a slow one: the loader's own guard restarts after the image and each row, so the wait does too
                    let p = o.dsp_progress();
                    if p != self.progress {
                        self.progress = p;
                        self.wait_restart(o);
                    } else if self.timed_out(o) {
                        // the DSP loader can take a load down (the way it does when a load fails): no transfer outlives the job
                        if o.cancel_dsp(self.rec, E_DSP_XFER) {
                            self.end(o, E_DSP_XFER, false);
                        } else {
                            // it ended meanwhile: the next pass reads its result
                            self.wait_restart(o);
                        }
                    }
                }
            }
        }
    }
}

/// The live job of the device.
static mut JOB: Job = Job::new();

fn job() -> &'static mut Job {
    unsafe { &mut *core::ptr::addr_of_mut!(JOB) }
}

/// The `RELOAD`, `LOAD` and `UNLOAD` commands and `dkrt_live`: queue job `op` for the module at `path` (see `Job::queue`).
pub unsafe fn cmd_queue(op: u32, path: *const u8) -> i32 {
    job().queue(&mut RealOps, op, path)
}

/// The job as `RELOAD_STATE` and `dkrt_live_state` report it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct State {
    pub phase: u8,
    pub result: i32,
    pub step: u8,
    pub torn: bool,
    pub op: u8,
}

/// The state of the job.
pub fn state() -> State {
    let j = job();
    State { phase: j.phase, result: j.result, step: j.step, torn: j.torn, op: j.op }
}

/// `dkrt_live(op, path)`: queue a live job from the DIGIKIT menu (the UI task). The same refusals as the SysEx commands: an open upload or a store write in flight is `E_STATE`.
#[no_mangle]
pub unsafe extern "C" fn dkrt_live(op: u32, path: *const u8) -> i32 {
    crate::init_once();
    if crate::store_writer::is_open() || crate::store_writer::is_locked() {
        return dkrt_sdk::proto::E_STATE;
    }
    cmd_queue(op, path)
}

/// `dkrt_live_state(out)`: the phase, result, step and teardown flag of the live job.
#[no_mangle]
pub unsafe extern "C" fn dkrt_live_state(out: *mut DkrtLive) {
    let s = state();
    *out = DkrtLive { phase: s.phase as u32, result: s.result, step: s.step as u32, unloaded: s.torn as u32, op: s.op as u32 };
}

/// One step of the job, once per UI pass (`dkrt_tick`).
pub unsafe fn poll() {
    if job().phase == 1 {
        job().poll(&mut RealOps);
    }
}

/// The job over the real system.
pub struct RealOps;

impl Ops for RealOps {
    unsafe fn find(&mut self, path: *const u8) -> Option<(usize, usize)> {
        crate::loader::rec_of_path(path)
    }
    unsafe fn is_service(&mut self, path: *const u8) -> bool {
        crate::loader::is_provider_path(path)
    }
    unsafe fn alloc_index(&mut self, path: *const u8) -> Result<usize, i32> {
        crate::boot::live_index(path, &crate::loader::live_indices())
    }
    unsafe fn refusal(&mut self, rec: Option<usize>) -> Option<i32> {
        let menus = match rec {
            Some(r) => core::ptr::read_volatile(core::ptr::addr_of!(crate::state::DKRT_MENU_OBJS[r])) != 0,
            None => false,
        };
        let f = Facts {
            safe_mode: crate::boot::info().safe != 0,
            upload_open: crate::store_writer::is_open() || crate::store_writer::is_locked(),
            card_busy: crate::api::card_busy(),
            menus,
        };
        // the generic refusals are the job's; the service-specific ones come from `can_stop` of the manager (each service's `teardown_refusal` for the module)
        let generic = refusal(&crate::PLATFORM, &f);
        match (generic, rec) {
            (None, Some(r)) => crate::lifecycle::get().and_then(|rt| rt.refusal(r)),
            (generic, _) => generic,
        }
    }
    unsafe fn check(&mut self, op: u32, rec: usize, k: usize, path: *const u8) -> i32 {
        if op == LIVE_LOAD {
            crate::loader::live_check_load(k, path)
        } else {
            crate::loader::reload_check(rec, path)
        }
    }
    fn phase(&mut self, owner: u8) {
        set_phase(owner)
    }
    unsafe fn quiesce(&mut self, rec: usize) -> i32 {
        crate::loader::reload_quiesce(rec)
    }
    unsafe fn abort(&mut self, rec: usize) -> bool {
        crate::loader::reload_abort(rec)
    }
    unsafe fn menu_built(&mut self, rec: usize) -> bool {
        rec < crate::MAX_MODULES && core::ptr::read_volatile(core::ptr::addr_of!(crate::state::DKRT_MENU_OBJS[rec])) != 0
    }
    unsafe fn unload(&mut self, rec: usize) -> i32 {
        match crate::lifecycle::get() {
            Some(rt) => rt.unload(rec),
            None => dkrt_sdk::E_NOT_FOUND,
        }
    }
    unsafe fn dsp_retained(&mut self) -> bool {
        crate::loader::dsp_retained()
    }
    unsafe fn dsp_progress(&mut self) -> u32 {
        crate::loader::dsp_progress()
    }
    unsafe fn begin_load(&mut self, k: usize) {
        crate::boot::begin_live(k);
    }
    unsafe fn load(&mut self, op: u32, rec: usize, k: usize, path: *const u8) -> i32 {
        let r = match crate::lifecycle::get() {
            Some(rt) if op == LIVE_LOAD => rt.live_load(k, path),
            Some(rt) => rt.reload_load(rec, path),
            None => dkrt_sdk::E_NOT_FOUND,
        };
        let missing = if r == E_IMPORT { crate::loader::missing() } else { [0u8; dkrt_sdk::NEEDS_LEN] };
        crate::boot::states().finished(k, r);
        crate::boot::report_module(k, path, r, &missing, &crate::loader::last_name());
        r
    }
    unsafe fn unloaded(&mut self, k: usize) {
        crate::loader::retire_index(k);
        crate::boot::unloaded(k);
    }
    unsafe fn retire(&mut self, k: usize) {
        crate::loader::retire_index(k);
        crate::boot::restore_live(k);
    }
    unsafe fn dsp_result(&mut self, k: usize) -> Option<i32> {
        let b = crate::boot::info();
        let s = b.states[k];
        if s == dkrt_sdk::STATE_ACTIVE || s == dkrt_sdk::STATE_FAILED {
            Some(b.results[k])
        } else {
            None
        }
    }
    unsafe fn cancel_dsp(&mut self, rec: usize, code: i32) -> bool {
        match crate::lifecycle::get() {
            Some(rt) => rt.cancel_dsp(rec, code),
            None => false,
        }
    }
    unsafe fn on_loaded(&mut self, rec: usize) {
        crate::handlers::services().on_loaded(rec)
    }
    unsafe fn finish(&mut self, ok: bool, changed: bool) {
        crate::handlers::services().live_done();
        if ok || changed {
            note_reloaded();
        }
    }
    fn blocks(&mut self) -> Option<u32> {
        crate::platform::fw_blocks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::FakePlatform;

    fn code(p: &FakePlatform, f: Facts) -> Option<i32> {
        unsafe { refusal(p, &f) }
    }
    fn all() -> Facts {
        Facts {
            safe_mode: true,
            upload_open: true,
            card_busy: true,
            menus: true,
        }
    }

    #[test]
    fn a_stopped_sequencer_and_no_other_fact_allows_the_reload() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        assert_eq!(code(&p, Facts::default()), None);
    }

    #[test]
    fn playing_paused_mixed_or_unreadable_transport_refuses_the_reload() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        for s in [Some(1), Some(2), Some(3), None] {
            p.play.set(s);
            assert_eq!(code(&p, Facts::default()), Some(E_RELOAD_PLAYING), "{s:?}");
        }
    }

    #[test]
    fn the_refusals_come_in_the_documented_order() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        p.play.set(Some(1));
        let mut f = all();
        assert_eq!(code(&p, f), Some(E_SAFE_MODE));
        f.safe_mode = false;
        assert_eq!(code(&p, f), Some(E_RELOAD_PLAYING));
        p.play.set(Some(0));
        assert_eq!(code(&p, f), Some(dkrt_sdk::proto::E_STATE));
        f.upload_open = false;
        assert_eq!(code(&p, f), Some(E_CARD_BUSY));
        f.card_busy = false;
        assert_eq!(code(&p, f), Some(E_RELOAD_MENUS));
        f.menus = false;
        assert_eq!(code(&p, f), None);
    }

    use crate::core_api::{CoreApi, REAL_CORE};
    use dkrt_abi::service::{RowPtrs, Service, ServiceRow, ServiceTable};

    /// A service that refuses the reload of module owner 2 only.
    struct Refuser;
    impl<C: CoreApi> Service<C> for Refuser {
        dkrt_abi::accepts_records!();
        unsafe fn teardown_refusal(_core: &C, owner: u8) -> i32 {
            if owner == 2 {
                E_RELOAD_ROUTED
            } else {
                OK
            }
        }
    }
    struct Quiet;
    impl<C: CoreApi> Service<C> for Quiet {
        dkrt_abi::accepts_records!();
        dkrt_abi::accepts_teardown!();
    }
    static ROWS: [ServiceRow; 2] = [ServiceRow::of::<Quiet, crate::core_api::RealCore>("quiet", &[]), ServiceRow::of::<Refuser, crate::core_api::RealCore>("refuser", &[])];
    static ROWS_P: RowPtrs<2> = RowPtrs::of(&ROWS);

    #[test]
    fn the_services_refuse_after_the_generic_refusals_and_only_for_their_module() {
        let p = FakePlatform::new(dkrt_abi::profiles::ALL[0]);
        let table = ServiceTable::new(&ROWS_P.0, &REAL_CORE);
        let go = |f: Facts, owner: u8| unsafe { refusal_with_services(&p, &f, &table, owner) };
        assert_eq!(go(Facts::default(), 1), None);
        assert_eq!(go(Facts::default(), 2), Some(E_RELOAD_ROUTED));
        // a generic refusal comes first
        assert_eq!(go(Facts { menus: true, ..Facts::default() }, 2), Some(E_RELOAD_MENUS));
        p.play.set(Some(1));
        assert_eq!(go(Facts::default(), 2), Some(E_RELOAD_PLAYING));
    }

    // ---- the job over a scripted system ------------------------------------------------------------------------------------------

    /// What the fake system answers, and what the job asked of it, in order.
    struct Script {
        found: Option<(usize, usize)>,
        /// The index a load is given (`Ops::alloc_index`).
        alloc: Result<usize, i32>,
        refusal: Option<i32>,
        check: i32,
        /// `quiesce` answers `E_BUSY` this many times, then `OK` (`u32::MAX`: always busy).
        quiesce_busy: u32,
        /// `unload` answers `E_BUSY` this many times, then `OK`.
        unload_busy: u32,
        /// Whether the old module can still be given back (`fini` has not run).
        can_abort: bool,
        load: i32,
        /// The DSP part answers `None` this many polls, then this result.
        dsp: (u32, i32),
        /// Whether `cancel_dsp` finds a load to take down.
        cancels: bool,
        service: bool,
        torn_seen: bool,
        /// The DSP loader holds a module it could not release.
        retained: bool,
        /// `dsp_progress` moves on every call (a slow load that goes on).
        progressing: bool,
        progress: u32,
        log: Vec<&'static str>,
        phase_on: bool,
        now: Option<u32>,
        /// The block clock never advances.
        stalled: bool,
        /// `menu_built` answers true from this many calls on (`u32::MAX`: never).
        menu_after: u32,
    }

    impl Script {
        fn new() -> Script {
            Script { found: Some((2, 5)), alloc: Ok(5), refusal: None, check: OK, quiesce_busy: 0, unload_busy: 0, can_abort: true, load: OK, dsp: (0, OK), cancels: true, service: false, torn_seen: false, retained: false, progressing: false, progress: 0, log: Vec::new(), phase_on: false, now: Some(0), stalled: false, menu_after: u32::MAX }
        }
    }

    impl Ops for Script {
        unsafe fn find(&mut self, _: *const u8) -> Option<(usize, usize)> {
            self.log.push("find");
            self.found
        }
        unsafe fn is_service(&mut self, _: *const u8) -> bool {
            self.service
        }
        unsafe fn dsp_retained(&mut self) -> bool {
            self.retained
        }
        unsafe fn dsp_progress(&mut self) -> u32 {
            if self.progressing {
                self.progress += 1;
            }
            self.progress
        }
        unsafe fn cancel_dsp(&mut self, _: usize, _: i32) -> bool {
            self.log.push("cancel dsp");
            self.cancels
        }
        unsafe fn alloc_index(&mut self, _: *const u8) -> Result<usize, i32> {
            self.log.push("alloc");
            self.alloc
        }
        unsafe fn refusal(&mut self, rec: Option<usize>) -> Option<i32> {
            self.log.push(if rec.is_some() { "refusal" } else { "refusal load" });
            self.refusal
        }
        unsafe fn check(&mut self, _: u32, _: usize, _: usize, _: *const u8) -> i32 {
            self.log.push("check");
            self.check
        }
        fn phase(&mut self, owner: u8) {
            self.phase_on = owner != 0;
            self.log.push(if owner != 0 { "phase on" } else { "phase off" });
        }
        unsafe fn quiesce(&mut self, _: usize) -> i32 {
            self.log.push("quiesce");
            if self.quiesce_busy == 0 {
                OK
            } else {
                if self.quiesce_busy != u32::MAX {
                    self.quiesce_busy -= 1;
                }
                E_BUSY
            }
        }
        unsafe fn abort(&mut self, _: usize) -> bool {
            self.log.push("abort");
            self.can_abort
        }
        unsafe fn menu_built(&mut self, _: usize) -> bool {
            if self.menu_after == 0 {
                return true;
            }
            if self.menu_after != u32::MAX {
                self.menu_after -= 1;
            }
            false
        }
        unsafe fn unload(&mut self, _: usize) -> i32 {
            self.log.push("unload");
            if self.unload_busy == 0 {
                OK
            } else {
                self.unload_busy -= 1;
                E_BUSY
            }
        }
        unsafe fn begin_load(&mut self, _: usize) {
            self.log.push("begin load");
        }
        unsafe fn load(&mut self, op: u32, _: usize, k: usize, _: *const u8) -> i32 {
            self.log.push("load");
            // the loader chose a record for a load that got as far as a record
            if op == LIVE_LOAD && (self.load == OK || self.load == LOADING) {
                self.found = Some((2, k));
            }
            self.load
        }
        unsafe fn unloaded(&mut self, _: usize) {
            self.log.push("unloaded");
        }
        unsafe fn retire(&mut self, _: usize) {
            self.log.push("retire");
        }
        unsafe fn dsp_result(&mut self, _: usize) -> Option<i32> {
            self.log.push("dsp");
            if self.dsp.0 == 0 {
                Some(self.dsp.1)
            } else {
                self.dsp.0 -= 1;
                None
            }
        }
        unsafe fn on_loaded(&mut self, _: usize) {
            self.log.push("on_loaded");
        }
        unsafe fn finish(&mut self, ok: bool, changed: bool) {
            self.log.push(if ok { "finish ok" } else if changed { "finish torn" } else { "finish failed" });
        }
        fn blocks(&mut self) -> Option<u32> {
            self.now
        }
    }

    /// Run a reload over `s` until it ends (at most 100000 polls); the final state.
    fn run(s: &mut Script) -> (u8, i32, u8) {
        run_op(s, LIVE_RELOAD)
    }

    /// Run a load, an unload or a reload over `s` until it ends (at most 100000 polls); the final state. A load starts from a system that has no module of that path.
    fn run_op(s: &mut Script, op: u32) -> (u8, i32, u8) {
        let mut j = Job::new();
        if op == LIVE_LOAD {
            s.found = None;
        }
        unsafe {
            assert_eq!(j.queue(s, op, b"mods/a.dkm\0".as_ptr()), OK);
            let mut n = 0;
            while j.phase == 1 && n < 100_000 {
                j.poll(s);
                if let (Some(t), false) = (s.now.as_mut(), s.stalled) {
                    *t += 100;
                }
                n += 1;
            }
        }
        s.torn_seen = j.torn;
        (j.phase, j.result, j.step)
    }

    #[test]
    fn a_reload_checks_drains_unloads_loads_and_ends_with_the_services_told_once() {
        let mut s = Script::new();
        s.quiesce_busy = 3;
        s.unload_busy = 2;
        assert_eq!(run(&mut s), (2, OK, step::DONE));
        let order: Vec<&str> = s.log.clone();
        let first = |name: &str| order.iter().position(|l| *l == name).unwrap();
        // nothing changes before the refusals and the check; the phase is on from there to the end; the services hear the end once, after the load
        assert!(first("find") < first("phase on") && first("phase on") < first("refusal") && first("refusal") < first("check"));
        assert!(first("check") < first("quiesce") && first("quiesce") < first("unload") && first("unload") < first("begin load") && first("begin load") < first("load"));
        assert!(first("load") < first("on_loaded") && first("on_loaded") < first("finish ok") && first("finish ok") < first("phase off"));
        assert_eq!(order.iter().filter(|l| **l == "finish ok").count(), 1);
        assert_eq!(order.iter().filter(|l| **l == "quiesce").count(), 4, "a busy drain is asked again on the next pass");
        assert!(!s.phase_on);
    }

    /// The end state of a reload that failed after the old module was torn down, in the log of the job: the services were told (`finish torn`, which frees the placeholders,
    /// writes the gap rows and rebuilds the list), the phase is off, and the job says the module is not loaded (`torn`).
    fn assert_not_loaded(s: &Script, what: &str) {
        assert!(s.torn_seen, "{what}: the job does not say the module is gone");
        assert_eq!(s.log.iter().filter(|l| **l == "finish torn").count(), 1, "{what}");
        assert!(!s.log.contains(&"finish ok") && !s.log.contains(&"on_loaded"), "{what}");
        assert_eq!(s.log.last(), Some(&"phase off"), "{what}");
        assert!(!s.phase_on, "{what}");
    }

    #[test]
    fn a_module_with_a_dsp_part_waits_for_it_and_gets_on_loaded_from_the_dsp_loader_not_the_job() {
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (3, OK);
        assert_eq!(run(&mut s), (2, OK, step::DONE));
        assert_eq!(s.log.iter().filter(|l| **l == "dsp").count(), 4);
        assert!(!s.log.contains(&"on_loaded"));
        assert_eq!(s.log.last(), Some(&"phase off"));
        // its DSP part fails: the module is not loaded, the services are told, the phase is off
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (1, E_DSP_XFER);
        assert_eq!(run(&mut s), (2, E_DSP_XFER, step::DONE));
        assert_not_loaded(&s, "a failed DSP part");
    }

    #[test]
    fn a_dsp_part_that_does_not_end_is_cancelled_and_the_module_is_not_loaded() {
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (u32::MAX, OK);
        assert_eq!(run(&mut s), (2, E_DSP_XFER, step::DONE));
        assert!(s.log.contains(&"cancel dsp"), "the transfer is taken down before the job ends");
        assert_not_loaded(&s, "a DSP part that timed out");
        // a load that ended while the job looked away is read, not cancelled twice
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (u32::MAX, OK);
        s.cancels = false;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
            let mut n = 0;
            while j.step != step::DSP || n < 3 {
                j.poll(&mut s);
                n += 1;
                if n > 50 {
                    break;
                }
            }
            for _ in 0..WAIT_PASSES + 5 {
                j.poll(&mut s);
            }
            assert_eq!(j.phase, 1, "cancel found nothing: the job keeps asking for the result");
            s.dsp = (0, OK);
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, OK));
    }

    #[test]
    fn a_refusal_or_a_failed_check_ends_the_job_before_anything_changes() {
        for (refusal, check, want) in [(Some(E_RELOAD_PLAYING), OK, E_RELOAD_PLAYING), (Some(dkrt_sdk::proto::E_STATE), OK, dkrt_sdk::proto::E_STATE), (None, E_RELOAD_KEYS, E_RELOAD_KEYS)] {
            let mut s = Script::new();
            s.refusal = refusal;
            s.check = check;
            assert_eq!(run(&mut s), (2, want, step::DONE));
            assert!(!s.torn_seen);
            assert!(!s.phase_on, "{want}: the phase is off again");
            for changed in ["quiesce", "unload", "load", "finish ok", "finish failed", "finish torn"] {
                assert!(!s.log.contains(&changed), "{want}: {changed} ran");
            }
        }
        // the module is looked up again at the check: one that went away since the queue is `E_NOT_FOUND`
        let mut s = Script::new();
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
            s.found = None;
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_NOT_FOUND));
    }

    #[test]
    fn a_drain_that_does_not_end_aborts_cleanly_and_leaves_the_old_module_live() {
        let mut s = Script::new();
        s.quiesce_busy = u32::MAX;
        assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
        assert!(s.log.contains(&"abort") && !s.log.contains(&"unload") && !s.log.contains(&"load"));
        assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
        assert!(!s.phase_on && !s.torn_seen);
        // without a block clock the timeout counts passes
        let mut s = Script::new();
        s.now = None;
        s.quiesce_busy = u32::MAX;
        assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
        // a block clock that stands still (the DSP stopped) does not hold the wait up either
        let mut s = Script::new();
        s.stalled = true;
        s.quiesce_busy = u32::MAX;
        assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
        assert!(s.log.contains(&"abort") && !s.log.contains(&"unload"));
        assert!(!s.phase_on && !s.torn_seen);
    }

    #[test]
    fn a_teardown_that_does_not_end_gives_the_old_module_back_because_unload_answers_busy_only_before_fini() {
        let mut s = Script::new();
        s.unload_busy = u32::MAX;
        assert_eq!(run(&mut s), (2, E_BUSY, step::DONE));
        assert!(s.log.contains(&"abort") && !s.log.contains(&"load"));
        assert!(!s.torn_seen, "before fini the old module is live");
        assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
        assert!(!s.phase_on);
    }

    /// The drain lasts many passes: a menu built for the module meanwhile (its settings page opened) ends the job before the teardown, for an unload and a reload, and the old module stays.
    #[test]
    fn a_menu_built_during_the_drain_gives_the_old_module_back() {
        for op in [LIVE_RELOAD, LIVE_UNLOAD] {
            let mut s = Script::new();
            s.quiesce_busy = 3;
            s.menu_after = 2; // not at the first pass of the unload step, then yes
            s.unload_busy = 5;
            assert_eq!(run_op(&mut s, op), (2, E_RELOAD_MENUS, step::DONE), "op {op}");
            assert!(s.log.contains(&"abort") && !s.log.contains(&"load"));
            assert!(!s.torn_seen);
            assert!(!s.phase_on);
            assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
        }
    }

    #[test]
    fn a_retained_dsp_refuses_at_the_queue_and_in_a_running_job_at_once() {
        // at the queue: no job, the code that says to restart
        let mut s = Script::new();
        s.retained = true;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), E_RELOAD_RETAINED);
            assert_eq!(j.phase, 0);
        }
        // in a job that was queued before the DSP was retained: the check refuses
        let mut s = Script::new();
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
            s.retained = true;
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
        assert!(!s.phase_on && !s.log.contains(&"quiesce"));
        // a teardown that fails because the DSP rows cannot be cleared: the DSP is retained, the job does not wait out its budget
        let mut s = Script::new();
        s.unload_busy = u32::MAX;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
            for _ in 0..4 {
                j.poll(&mut s);
            }
            s.retained = true;
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
        assert!(s.log.contains(&"abort") && !j.torn);
    }

    #[test]
    fn a_slow_dsp_load_that_goes_on_is_not_cancelled_and_a_stalled_one_is() {
        // ten rows at 17 passes each, and a long image: far more passes than one budget, but the loader moves all the time
        let mut s = Script::new();
        s.load = LOADING;
        s.progressing = true;
        s.dsp = (10 * 17 * 40, OK);
        assert_eq!(run(&mut s), (2, OK, step::DONE));
        assert!(!s.log.contains(&"cancel dsp"));
        // the loader stands still: cancelled after one budget
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (u32::MAX, OK);
        assert_eq!(run(&mut s), (2, E_DSP_XFER, step::DONE));
        assert!(s.log.contains(&"cancel dsp"));
        // progress that stops half way is cancelled one budget after it stopped
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (u32::MAX, OK);
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"mods/a.dkm\0".as_ptr()), OK);
            s.progressing = true;
            for _ in 0..3000 {
                j.poll(&mut s);
            }
            assert_eq!(j.phase, 1, "a load that moves is left alone");
            s.progressing = false;
            for _ in 0..WAIT_PASSES + 10 {
                j.poll(&mut s);
            }
        }
        assert_eq!((j.phase, j.result), (2, E_DSP_XFER));
    }

    #[test]
    fn a_load_that_fails_ends_with_its_code_and_the_module_is_not_loaded() {
        let mut s = Script::new();
        s.load = E_INIT;
        assert_eq!(run(&mut s), (2, E_INIT, step::DONE));
        assert_not_loaded(&s, "a failed load");
    }

    /// The wire mapping of the live commands (`admin_cmds`): the queue's refusal is the reply status, and `RELOAD_STATE` carries phase, result, step, torn and op of the job as bytes.
    /// The end of a job counts in `DkrtBootInfo.reloads` when the module set changed: a finished job (`ok`) or one that tore a module down (`changed`), not a refusal.
    #[test]
    fn a_job_that_changed_the_module_set_counts_one_reload_and_a_refusal_counts_none() {
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            let n = || crate::boot::info().reloads;
            let start = n();
            RealOps.finish(false, false);
            assert_eq!(n(), start);
            RealOps.finish(true, false);
            assert_eq!(n(), start.wrapping_add(1));
            RealOps.finish(false, true);
            assert_eq!(n(), start.wrapping_add(2));
        }
    }

    #[test]
    fn the_live_commands_answer_with_the_jobs_refusal_and_state_in_the_reload_state_body() {
        use crate::admin_cmds::{cmd_load, cmd_reload, cmd_reload_state, cmd_unload};
        use crate::transport::{DKRT_UPLOAD_LAST, RBODY};
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let status = || unsafe { core::ptr::read_volatile(core::ptr::addr_of!(DKRT_UPLOAD_LAST)) };
        let body = |i: usize| unsafe { *(core::ptr::addr_of!(RBODY) as *const u8).add(1 + i) };
        unsafe {
            *job() = Job::new();
            let path = b"mods/x.dkm\0";
            // no loaded module came from the path: reload and unload say E_NOT_FOUND, an empty body is E_MSG, and no job is queued
            cmd_reload(1, path.as_ptr(), path.len());
            assert_eq!(status(), E_NOT_FOUND);
            cmd_unload(1, path.as_ptr(), path.len());
            assert_eq!(status(), E_NOT_FOUND);
            cmd_load(1, b"\0".as_ptr(), 1);
            assert_eq!(status(), dkrt_sdk::proto::E_MSG);
            cmd_reload_state(1);
            assert_eq!((status(), body(0), body(6), body(7)), (OK, 0, 0, 0), "no job yet");
            // a finished unload that was refused by playback: result, step, torn and op in the body
            *job() = Job { phase: 2, result: E_RELOAD_PLAYING, step: 5, torn: true, op: LIVE_UNLOAD as u8, ..Job::new() };
            cmd_reload_state(2);
            assert_eq!(status(), OK);
            assert_eq!(body(0), 2, "phase");
            assert_eq!(i32::from_be_bytes([body(1), body(2), body(3), body(4)]), E_RELOAD_PLAYING, "result");
            assert_eq!((body(5), body(6), body(7)), (5, 1, LIVE_UNLOAD as u8), "step, torn, op");
            *job() = Job::new();
        }
    }

    #[test]
    fn a_service_queues_a_live_job_and_reads_its_state_through_the_core_imports() {
        use crate::svc_imports::shims::{dkc_live, dkc_live_state};
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            // `dkrt_live` runs `init_once`, which sets up the arena at a 32-bit address that a 64-bit host cannot write: the runtime counts as initialised
            crate::st().magic = crate::state::STATE_MAGIC;
            *job() = Job::new();
            // the refusals are those of `dkrt_live`: no loaded module came from the path, an empty path, an unknown op; nothing is queued
            assert_eq!(dkc_live(LIVE_RELOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
            assert_eq!(dkc_live(LIVE_UNLOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
            assert_eq!(dkc_live(LIVE_RELOAD, b"\0".as_ptr()), dkrt_sdk::proto::E_MSG);
            assert_eq!(dkc_live(99, b"mods/x.dkm\0".as_ptr()), dkrt_sdk::proto::E_MSG);
            assert_eq!(job().phase, 0);
            // a queued job (the state a successful queue leaves) is read the way `dkrt_live_state` reports it
            *job() = Job { phase: 1, step: step::CHECK, op: LIVE_LOAD as u8, ..Job::new() };
            assert_eq!(dkc_live(LIVE_LOAD, b"mods/y.dkm\0".as_ptr()), E_BUSY, "one job at a time");
            let mut out = core::mem::MaybeUninit::<DkrtLive>::uninit();
            dkc_live_state(out.as_mut_ptr());
            let out = out.assume_init();
            assert_eq!((out.phase, out.step, out.op, out.unloaded), (1, step::CHECK as u32, LIVE_LOAD, 0));
            *job() = Job { phase: 2, result: E_RELOAD_PLAYING, step: 5, torn: true, op: LIVE_UNLOAD as u8, ..Job::new() };
            let mut out = core::mem::MaybeUninit::<DkrtLive>::uninit();
            dkc_live_state(out.as_mut_ptr());
            let out = out.assume_init();
            assert_eq!((out.phase, out.result, out.step, out.unloaded, out.op), (2, E_RELOAD_PLAYING, 5, 1, LIVE_UNLOAD));
            *job() = Job::new();
        }
    }

    #[test]
    fn a_service_cannot_load_unload_or_reload_the_file_of_a_loaded_service() {
        use crate::svc_imports::shims::dkc_live;
        let _g = crate::test_lock::LOCK.lock().unwrap_or_else(|e| e.into_inner());
        unsafe {
            crate::st().magic = crate::state::STATE_MAGIC;
            *job() = Job::new();
            // a loaded service: the first provider record, with its path
            let rec = crate::MAX_MODULES;
            let path = b"services/svc.dks\0";
            crate::loader::records::module(rec).used = 1;
            crate::loader::records::path(rec)[..path.len()].copy_from_slice(path);
            let answers = [LIVE_LOAD, LIVE_UNLOAD, LIVE_RELOAD].map(|op| dkc_live(op, path.as_ptr()));
            crate::loader::records::module(rec).used = 0;
            crate::loader::records::path(rec)[..path.len()].fill(0);
            assert_eq!(answers, [E_FORMAT; 3]);
            assert_eq!(job().phase, 0, "nothing is queued");
        }
    }

    #[test]
    fn queueing_resolves_the_path_refuses_a_service_file_a_bad_path_and_a_second_job() {
        let mut s = Script::new();
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"a\0".as_ptr()), OK);
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"b\0".as_ptr()), E_BUSY);
            let mut k = Job::new();
            assert_eq!(k.queue(&mut s, LIVE_RELOAD, b"\0".as_ptr()), dkrt_sdk::proto::E_MSG);
            let mut v = [b'x'; 70].to_vec();
            v.push(0);
            assert_eq!(k.queue(&mut s, LIVE_RELOAD, v.as_ptr()), dkrt_sdk::proto::E_MSG);
            // no loaded module came from the path: the reply says so at once, and no job is queued
            s.found = None;
            assert_eq!(k.queue(&mut s, LIVE_RELOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
            assert_eq!(k.queue(&mut s, LIVE_UNLOAD, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
            assert_eq!(k.phase, 0);
            // a load of a path that is loaded is `E_LOADED`; an operation that does not exist is `E_MSG`
            s.found = Some((2, 5));
            assert_eq!(k.queue(&mut s, LIVE_LOAD, b"mods/x.dkm\0".as_ptr()), E_LOADED);
            assert_eq!(k.queue(&mut s, 0, b"mods/x.dkm\0".as_ptr()), dkrt_sdk::proto::E_MSG);
            assert_eq!(k.queue(&mut s, 9, b"mods/x.dkm\0".as_ptr()), dkrt_sdk::proto::E_MSG);
            assert_eq!(k.phase, 0);
            // the file of a service is no module
            s.service = true;
            for op in [LIVE_LOAD, LIVE_UNLOAD, LIVE_RELOAD] {
                assert_eq!(k.queue(&mut s, op, b"services/fx.dks\0".as_ptr()), E_FORMAT);
            }
            assert_eq!(k.phase, 0);
            s.service = false;
            // a finished job takes the next one
            while j.phase == 1 {
                j.poll(&mut s);
            }
            assert_eq!(j.queue(&mut s, LIVE_RELOAD, b"c\0".as_ptr()), OK);
        }
    }

    // ---- load and unload (E5c) ------------------------------------------------------------------------------------------------

    fn first(s: &Script, name: &str) -> usize {
        s.log.iter().position(|l| *l == name).unwrap_or_else(|| panic!("{name} did not run: {:?}", s.log))
    }

    fn nothing_changed(s: &Script, what: &str) {
        for changed in ["begin load", "load", "quiesce", "unload", "unloaded", "retire", "finish ok", "finish failed", "finish torn"] {
            assert!(!s.log.contains(&changed), "{what}: {changed} ran: {:?}", s.log);
        }
        assert!(!s.phase_on && !s.torn_seen, "{what}");
    }

    #[test]
    fn a_load_checks_loads_and_ends_with_the_services_told_once_and_never_drains_or_unloads() {
        let mut s = Script::new();
        assert_eq!(run_op(&mut s, LIVE_LOAD), (2, OK, step::DONE));
        // the index, the refusals (those of a load: no module record), the file checks, then the load; no phase (a load has no rows of its own to leave out), no drain, no teardown
        assert!(first(&s, "find") < first(&s, "alloc") && first(&s, "alloc") < first(&s, "refusal load") && first(&s, "refusal load") < first(&s, "check"));
        assert!(first(&s, "check") < first(&s, "begin load") && first(&s, "begin load") < first(&s, "load") && first(&s, "load") < first(&s, "on_loaded"));
        assert!(first(&s, "on_loaded") < first(&s, "finish ok") && first(&s, "finish ok") < first(&s, "phase off"));
        for never in ["phase on", "refusal", "quiesce", "unload", "unloaded", "abort", "retire", "finish torn"] {
            assert!(!s.log.contains(&never), "{never}: {:?}", s.log);
        }
        assert!(!s.torn_seen, "a load tears nothing down");
        assert_eq!(s.log.iter().filter(|l| **l == "finish ok").count(), 1);
    }

    #[test]
    fn a_load_with_a_dsp_part_waits_for_it_like_a_reload() {
        let mut s = Script::new();
        s.load = LOADING;
        s.dsp = (3, OK);
        assert_eq!(run_op(&mut s, LIVE_LOAD), (2, OK, step::DONE));
        assert_eq!(s.log.iter().filter(|l| **l == "dsp").count(), 4);
        assert!(!s.log.contains(&"on_loaded") && !s.log.contains(&"retire"));
        assert_eq!(s.log.last(), Some(&"phase off"));
    }

    #[test]
    fn a_load_that_does_not_come_up_ends_as_it_began_with_the_index_free() {
        // the load fails
        let mut s = Script::new();
        s.load = E_INIT;
        assert_eq!(run_op(&mut s, LIVE_LOAD), (2, E_INIT, step::DONE));
        assert!(s.log.contains(&"retire") && !s.log.contains(&"on_loaded") && !s.log.contains(&"finish ok"));
        assert!(first(&s, "retire") < first(&s, "finish failed"));
        assert!(!s.torn_seen && !s.phase_on);
        // its DSP part fails, or stalls and is cancelled
        for (dsp, code) in [((1, E_DSP_XFER), E_DSP_XFER), ((u32::MAX, OK), E_DSP_XFER)] {
            let mut s = Script::new();
            s.load = LOADING;
            s.dsp = dsp;
            assert_eq!(run_op(&mut s, LIVE_LOAD), (2, code, step::DONE));
            assert!(s.log.contains(&"retire") && !s.log.contains(&"on_loaded"), "{:?}", s.log);
            assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
            assert!(!s.torn_seen && !s.phase_on);
        }
        // a reload that fails after its teardown keeps the module's index (the module is FAILED, not free): no retire
        let mut s = Script::new();
        s.load = E_INIT;
        assert_eq!(run_op(&mut s, LIVE_RELOAD), (2, E_INIT, step::DONE));
        assert!(!s.log.contains(&"retire"));
    }

    #[test]
    fn a_load_that_is_refused_or_fails_its_check_changes_nothing() {
        for (refusal, check, alloc, want) in [
            (Some(E_RELOAD_PLAYING), OK, Ok(5), E_RELOAD_PLAYING),
            (Some(E_SAFE_MODE), OK, Ok(5), E_SAFE_MODE),
            (Some(dkrt_sdk::proto::E_STATE), OK, Ok(5), dkrt_sdk::proto::E_STATE),
            (None, E_MISSING, Ok(5), E_MISSING),
            (None, E_PLAN_KEY, Ok(5), E_PLAN_KEY),
            (None, OK, Err(E_SEL_COUNT), E_SEL_COUNT),
        ] {
            let mut s = Script::new();
            s.refusal = refusal;
            s.check = check;
            s.alloc = alloc;
            assert_eq!(run_op(&mut s, LIVE_LOAD), (2, want, step::DONE), "{want}");
            nothing_changed(&s, &format!("{want}"));
        }
        // a module of that path came while the job waited: E_LOADED, still nothing changed
        let mut s = Script::new();
        let mut j = Job::new();
        s.found = None;
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_LOAD, b"mods/a.dkm\0".as_ptr()), OK);
            s.found = Some((2, 5));
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_LOADED));
        nothing_changed(&s, "E_LOADED");
    }

    #[test]
    fn a_load_into_a_retained_dsp_is_refused_at_the_queue_and_in_a_running_job() {
        let mut s = Script::new();
        s.retained = true;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_LOAD, b"mods/a.dkm\0".as_ptr()), E_RELOAD_RETAINED);
        }
        assert_eq!(j.phase, 0);
        let mut s = Script::new();
        s.found = None;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_LOAD, b"mods/a.dkm\0".as_ptr()), OK);
            s.retained = true;
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
        nothing_changed(&s, "retained");
    }

    #[test]
    fn an_unload_checks_the_module_drains_tears_down_and_ends_not_loaded_with_the_services_told_once() {
        let mut s = Script::new();
        s.quiesce_busy = 3;
        s.unload_busy = 2;
        assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, OK, step::DONE));
        // the refusals of a module (menus, services), no file check, no phase, no load half
        assert!(first(&s, "find") < first(&s, "refusal") && first(&s, "refusal") < first(&s, "quiesce"));
        assert!(first(&s, "quiesce") < first(&s, "unload") && first(&s, "unload") < first(&s, "unloaded") && first(&s, "unloaded") < first(&s, "finish ok"));
        assert!(first(&s, "finish ok") < first(&s, "phase off"));
        for never in ["alloc", "refusal load", "check", "phase on", "begin load", "load", "on_loaded", "dsp", "retire", "abort", "finish torn"] {
            assert!(!s.log.contains(&never), "{never}: {:?}", s.log);
        }
        assert!(s.torn_seen, "the module is gone");
        assert_eq!(s.log.iter().filter(|l| **l == "finish ok").count(), 1);
        assert_eq!(s.log.iter().filter(|l| **l == "unloaded").count(), 1);
        assert_eq!(s.log.iter().filter(|l| **l == "quiesce").count(), 4, "a busy drain is asked again on the next pass");
    }

    #[test]
    fn an_unload_that_is_refused_changes_nothing_and_one_of_a_module_that_is_gone_is_not_found() {
        for refusal in [E_RELOAD_PLAYING, E_RELOAD_MENUS, E_RELOAD_ROUTED, E_CARD_BUSY, dkrt_sdk::proto::E_STATE, E_SAFE_MODE] {
            let mut s = Script::new();
            s.refusal = Some(refusal);
            assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, refusal, step::DONE), "{refusal}");
            nothing_changed(&s, &format!("{refusal}"));
        }
        let mut s = Script::new();
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_UNLOAD, b"mods/a.dkm\0".as_ptr()), OK);
            s.found = None;
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_NOT_FOUND));
        nothing_changed(&s, "gone");
    }

    #[test]
    fn an_unload_whose_drain_or_teardown_does_not_end_gives_the_module_back() {
        // the drain: nothing was freed
        let mut s = Script::new();
        s.quiesce_busy = u32::MAX;
        assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, E_BUSY, step::DONE));
        assert!(s.log.contains(&"abort") && !s.log.contains(&"unload") && !s.log.contains(&"unloaded"));
        assert_eq!(s.log.iter().filter(|l| **l == "finish failed").count(), 1);
        assert!(!s.phase_on && !s.torn_seen);
        // the teardown stops before `fini`
        let mut s = Script::new();
        s.unload_busy = u32::MAX;
        assert_eq!(run_op(&mut s, LIVE_UNLOAD), (2, E_BUSY, step::DONE));
        assert!(s.log.contains(&"abort") && !s.log.contains(&"unloaded"));
        assert!(!s.torn_seen, "before fini the module is live");
        // the DSP rows cannot be cleared: retained, said at once
        let mut s = Script::new();
        s.unload_busy = u32::MAX;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, LIVE_UNLOAD, b"mods/a.dkm\0".as_ptr()), OK);
            for _ in 0..4 {
                j.poll(&mut s);
            }
            s.retained = true;
            j.poll(&mut s);
        }
        assert_eq!((j.phase, j.result), (2, E_RELOAD_RETAINED));
        assert!(!j.torn && !s.log.contains(&"unloaded"));
    }

    #[test]
    fn a_reload_does_not_free_its_index_and_the_state_names_the_operation() {
        let mut s = Script::new();
        assert_eq!(run(&mut s), (2, OK, step::DONE));
        assert!(!s.log.contains(&"unloaded") && !s.log.contains(&"retire") && !s.log.contains(&"alloc"));
        for (op, torn) in [(LIVE_LOAD, false), (LIVE_UNLOAD, true), (LIVE_RELOAD, true)] {
            let mut s = Script::new();
            if op == LIVE_LOAD {
                s.found = None;
            }
            let mut j = Job::new();
            unsafe {
                assert_eq!(j.queue(&mut s, op, b"mods/a.dkm\0".as_ptr()), OK);
                assert_eq!((j.op as u32, j.phase), (op, 1));
                while j.phase == 1 {
                    j.poll(&mut s);
                }
            }
            assert_eq!((j.op as u32, j.torn, j.result), (op, torn, OK), "op {op}");
        }
    }

    #[test]
    fn the_checks_of_a_reload_run_with_its_phase_on_and_those_of_a_load_and_an_unload_without() {
        for (op, phase) in [(LIVE_RELOAD, true), (LIVE_LOAD, false), (LIVE_UNLOAD, false)] {
            let mut s = Script::new();
            assert_eq!(run_op(&mut s, op), (2, OK, step::DONE));
            assert_eq!(s.log.contains(&"phase on"), phase, "op {op}");
        }
    }
}
