//! The live job: one job with three operations (load, unload, reload), run by `poll` one step per UI pass over `Ops`.
use dkrt_sdk::*;

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
    /// `Rt::unload`: the manager takes the module and its consumers down.
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
    /// A job in a given state, for the tests of `state` and `poll`'s readers.
    #[cfg(test)]
    pub(super) fn in_state(step: u8, phase: u8, result: i32, torn: bool, op: u8) -> Job {
        Job { step, phase, result, torn, op, ..Job::new() }
    }

    /// UI passes a wait may take before the job gives up.
    #[cfg(test)]
    pub(super) const fn wait_passes() -> u32 {
        WAIT_PASSES
    }

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
        let n = dkrt_abi::mem::cstr_len(path, 64);
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
                // `Rt::unload` answers E_BUSY only before `fini` (a file or I/O step of the module still runs, or the DSP rows cannot be cleared), so the old module is whole and gets
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
