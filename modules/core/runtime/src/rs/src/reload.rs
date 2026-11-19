//! Reload of a content module (docs/plans/extension-model.md E5): what the runtime holds once for every kind of module. This file has the refusal order;
//! the job, the loader steps and the DSP wait follow in the other files of E5. Per-kind work sits behind the `Service` trait, never in here.

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
/// (`CoreApi::reload_phase`). The reload job sets and clears it; a service reads it where the module's own rows must not count against the new file (the
/// checks of its types and rows) and where its teardown differs from a plain unload (the placeholder rows).
static mut PHASE: u32 = 0;

/// The reload phase now.
pub fn phase() -> u32 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PHASE)) }
}

/// Start the reload phase of module `owner` (record index + 1), or end it (0). Only the reload job calls it.
pub(crate) fn set_phase(owner: u8) {
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PHASE), owner as u32) }
}

/// A reload finished: the counter that tells a host and the menus their lists of modules and machines are out of date (`DkrtBootInfo.reloads`).
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
    /// The module built menus of its own (its header names a menu entry): they keep pointers into its code.
    pub menus: bool,
}

/// The refusal of a reload, or `None` when it may go on. One function, one order: safe mode, the firmware's transport (playing, paused, or not
/// readable), an open upload, card operations, menus of the module. The first that holds names the status. The services' own refusals (`Service::reload_refusal`, for
/// example an effect on a route) come after it: `refusal_with_services`.
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

/// `refusal`, then the refusals of the services about module record `owner - 1` (`Service::reload_refusal`, every service in table order). The reload job calls this.
pub unsafe fn refusal_with_services<P: Platform>(p: &P, f: &Facts, services: &dkrt_abi::service::ServiceTable, owner: u8) -> Option<i32> {
    if let Some(r) = refusal(p, f) {
        return Some(r);
    }
    let r = services.reload_refusal(owner);
    if r != OK {
        return Some(r);
    }
    None
}


// ---- the job ----------------------------------------------------------------------------------------------------------------------------------

/// What the reload job asks of the system, one call per step. The real one (`RealOps`) calls the loader, the services and the boot record; a host test scripts it.
pub trait Ops {
    /// The module record and selection index of the loaded content module that came from `path`.
    unsafe fn find(&mut self, path: *const u8) -> Option<(usize, usize)>;
    /// Is `path` the file of a loaded service (a provider record)? A reload is for content modules only.
    unsafe fn is_service(&mut self, path: *const u8) -> bool;
    /// The refusals before anything changes: `reload::refusal_with_services` over the facts of the system (safe mode, an open upload or a card transfer, menus).
    unsafe fn refusal(&mut self, rec: usize) -> Option<i32>;
    /// `Loader::reload_check`.
    unsafe fn check(&mut self, rec: usize, path: *const u8) -> i32;
    /// The reload phase (`CoreApi::reload_phase`) of the module with this owner (record index + 1), or off (0).
    fn phase(&mut self, owner: u8);
    /// `Loader::reload_quiesce`.
    unsafe fn quiesce(&mut self, rec: usize) -> i32;
    /// `Loader::reload_abort`: the old module gets its tick and commands back; false when its `fini` ran.
    unsafe fn abort(&mut self, rec: usize) -> bool;
    /// `Loader::unload_rec`.
    unsafe fn unload(&mut self, rec: usize) -> i32;
    /// Does the DSP loader hold a module it could not release? Permanent until the next boot.
    unsafe fn dsp_retained(&mut self) -> bool;
    /// How far the DSP loader has got with its loads (a counter that moves with every image or row it sends and every load it starts).
    unsafe fn dsp_progress(&mut self) -> u32;
    /// The boot record's view of the module goes to INIT (and the loaded count drops if it was ACTIVE).
    unsafe fn begin_load(&mut self, k: usize);
    /// `Loader::reload_load` and the boot record's account of its result.
    unsafe fn load(&mut self, rec: usize, k: usize, path: *const u8) -> i32;
    /// The DSP part of the new file: `None` while it goes on, else its result (the boot record's `results[k]`).
    unsafe fn dsp_result(&mut self, k: usize) -> Option<i32>;
    /// Give up the DSP part (`Loader::cancel_dsp`): the transfer is taken down as a failed one is, and the module is torn down.
    unsafe fn cancel_dsp(&mut self, rec: usize, code: i32) -> bool;
    /// Every service: `Service::on_loaded` of module record `rec` (a module with no DSP gets it here; one with DSP got it from the DSP loader).
    unsafe fn on_loaded(&mut self, rec: usize);
    /// The end of a reload that got past the checks, whatever the result: `Service::reload_done` on every service (placeholders freed, the machine rows of a module that is not
    /// back written as gap rows, the machine list rebuilt), and the reload counter when the module was loaded or torn down (`changed`). A refusal before `set_phase` skips it:
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

/// One reload at a time. `poll` runs one step per UI pass and never waits.
pub struct Job {
    pub step: u8,
    /// 0 none yet, 1 queued or running, 2 finished.
    pub phase: u8,
    /// The load status of the new file, or the refusal.
    pub result: i32,
    /// The old module was torn down (its `fini` ran): the module is not loaded unless the new file loaded. A refusal or an abort before that leaves the old module running.
    pub torn: bool,
    path: [u8; 64],
    rec: usize,
    sel: usize,
    start: Option<u32>,
    passes: u32,
    /// `Ops::dsp_progress` at the last pass of the DSP wait.
    progress: u32,
}

impl Job {
    pub const fn new() -> Job {
        Job { step: step::IDLE, phase: 0, result: OK, torn: false, path: [0; 64], rec: 0, sel: 0, start: None, passes: 0, progress: 0 }
    }

    /// Queue a reload of the module loaded from `path` (NUL terminated, at most 63 bytes). `OK`; `E_BUSY` while one is queued or running; `E_MSG` for a path that does not
    /// fit; `E_RELOAD_RETAINED` while the DSP loader holds a module it could not release (restart the device); `E_FORMAT` for the file of a service; `E_NOT_FOUND` when no loaded
    /// content module came from that path.
    pub unsafe fn queue<O: Ops>(&mut self, o: &mut O, path: *const u8) -> i32 {
        if self.phase == 1 {
            return E_BUSY;
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
        if o.find(path).is_none() {
            return E_NOT_FOUND;
        }
        let mut i = 0;
        while i < 64 {
            self.path[i] = if i < n { *path.add(i) } else { 0 };
            i += 1;
        }
        self.phase = 1;
        self.step = step::CHECK;
        self.result = OK;
        self.torn = false;
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

    /// The job ends with `result` after `set_phase`: the services' end of a reload, the phase off.
    unsafe fn end<O: Ops>(&mut self, o: &mut O, result: i32, ok: bool) {
        o.finish(ok, self.torn);
        o.phase(0);
        self.result = result;
        self.step = step::DONE;
        self.phase = 2;
    }

    /// One step. Called once per UI pass (`dkrt_tick`).
    pub unsafe fn poll<O: Ops>(&mut self, o: &mut O) {
        if self.phase != 1 {
            return;
        }
        let path = self.path.as_ptr();
        if self.step == step::CHECK {
            // nothing changes before these pass: a refusal ends the job with the old module as it was. The path is looked up again (the module may be gone since the queue).
            let (rec, k) = match o.find(path) {
                Some(f) => f,
                None => return self.refused(E_NOT_FOUND),
            };
            self.rec = rec;
            self.sel = k;
            // the phase is on for the checks too: the services leave the module's own rows out of "used by another module" (types, rows)
            o.phase(rec as u8 + 1);
            if o.dsp_retained() {
                o.phase(0);
                return self.refused(E_RELOAD_RETAINED);
            }
            if let Some(r) = o.refusal(rec) {
                o.phase(0);
                return self.refused(r);
            }
            let r = o.check(rec, path);
            if r != OK {
                o.phase(0);
                return self.refused(r);
            }
            self.wait_restart(o);
            self.step = step::QUIESCE;
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
            let r = o.unload(self.rec);
            if r == OK {
                self.torn = true;
                o.begin_load(self.sel);
                self.step = step::LOAD;
            } else if r != E_BUSY || o.dsp_retained() || self.timed_out(o) {
                // `unload_rec` answers E_BUSY only before `fini` (a file or I/O step of the module still runs, or the DSP rows cannot be cleared), so the old module is whole and gets
                // its tick and commands back. A DSP loader that holds rows it could not clear is permanent: say so at once instead of waiting out the budget.
                o.abort(self.rec);
                let retained = o.dsp_retained();
                self.end(o, if retained { E_RELOAD_RETAINED } else { r }, false);
            }
        } else if self.step == step::LOAD {
            let r = o.load(self.rec, self.sel, path);
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

/// The reload job of the device.
static mut JOB: Job = Job::new();

fn job() -> &'static mut Job {
    unsafe { &mut *core::ptr::addr_of_mut!(JOB) }
}

/// The `RELOAD` command: queue the reload of the module loaded from `path` (see `Job::queue`).
pub unsafe fn cmd_queue(path: *const u8) -> i32 {
    job().queue(&mut RealOps, path)
}

/// `(phase, result, step, torn)` for `RELOAD_STATE`.
pub fn state() -> (u8, i32, u8, bool) {
    let j = job();
    (j.phase, j.result, j.step, j.torn)
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
    unsafe fn refusal(&mut self, rec: usize) -> Option<i32> {
        let f = Facts {
            safe_mode: crate::boot::info().safe != 0,
            upload_open: crate::store_writer::is_open() || crate::store_writer::is_locked(),
            card_busy: crate::api::card_busy(),
            menus: core::ptr::read_volatile(core::ptr::addr_of!(crate::state::DKRT_MENUS[rec])) != 0,
        };
        refusal_with_services(&crate::PLATFORM, &f, &crate::handlers::services(), rec as u8 + 1)
    }
    unsafe fn check(&mut self, rec: usize, path: *const u8) -> i32 {
        crate::loader::reload_check(rec, path)
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
    unsafe fn unload(&mut self, rec: usize) -> i32 {
        crate::loader::unload_rec(rec)
    }
    unsafe fn dsp_retained(&mut self) -> bool {
        crate::loader::dsp_retained()
    }
    unsafe fn dsp_progress(&mut self) -> u32 {
        crate::loader::dsp_progress()
    }
    unsafe fn begin_load(&mut self, k: usize) {
        let b = crate::boot::info();
        if b.states[k] == dkrt_sdk::STATE_ACTIVE && b.loaded > 0 {
            b.loaded -= 1;
        }
        b.states[k] = dkrt_sdk::STATE_INIT;
    }
    unsafe fn load(&mut self, rec: usize, k: usize, path: *const u8) -> i32 {
        let r = crate::loader::reload_load(rec, path);
        let missing = [0u8; dkrt_sdk::NEEDS_LEN];
        crate::boot::states().finished(k, r);
        crate::boot::report_module(k, path, r, &missing);
        r
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
        crate::loader::cancel_dsp(rec, code)
    }
    unsafe fn on_loaded(&mut self, rec: usize) {
        crate::handlers::services().on_loaded(rec)
    }
    unsafe fn finish(&mut self, ok: bool, changed: bool) {
        crate::handlers::services().reload_done();
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
        unsafe fn reload_refusal(_core: &C, owner: u8) -> i32 {
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
        dkrt_abi::accepts_reload!();
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
    }

    impl Script {
        fn new() -> Script {
            Script { found: Some((2, 5)), refusal: None, check: OK, quiesce_busy: 0, unload_busy: 0, can_abort: true, load: OK, dsp: (0, OK), cancels: true, service: false, torn_seen: false, retained: false, progressing: false, progress: 0, log: Vec::new(), phase_on: false, now: Some(0), stalled: false }
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
        unsafe fn refusal(&mut self, _: usize) -> Option<i32> {
            self.log.push("refusal");
            self.refusal
        }
        unsafe fn check(&mut self, _: usize, _: *const u8) -> i32 {
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
        unsafe fn load(&mut self, _: usize, _: usize, _: *const u8) -> i32 {
            self.log.push("load");
            self.load
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

    /// Run a job over `s` until it ends (at most 100000 polls); the log and the final state.
    fn run(s: &mut Script) -> (u8, i32, u8) {
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(s, b"mods/a.dkm\0".as_ptr()), OK);
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
            assert_eq!(j.queue(&mut s, b"mods/a.dkm\0".as_ptr()), OK);
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
            assert_eq!(j.queue(&mut s, b"mods/a.dkm\0".as_ptr()), OK);
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

    #[test]
    fn a_retained_dsp_refuses_at_the_queue_and_in_a_running_job_at_once() {
        // at the queue: no job, the code that says to restart
        let mut s = Script::new();
        s.retained = true;
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, b"mods/a.dkm\0".as_ptr()), E_RELOAD_RETAINED);
            assert_eq!(j.phase, 0);
        }
        // in a job that was queued before the DSP was retained: the check refuses
        let mut s = Script::new();
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, b"mods/a.dkm\0".as_ptr()), OK);
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
            assert_eq!(j.queue(&mut s, b"mods/a.dkm\0".as_ptr()), OK);
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
            assert_eq!(j.queue(&mut s, b"mods/a.dkm\0".as_ptr()), OK);
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

    #[test]
    fn queueing_resolves_the_path_refuses_a_service_file_a_bad_path_and_a_second_job() {
        let mut s = Script::new();
        let mut j = Job::new();
        unsafe {
            assert_eq!(j.queue(&mut s, b"a\0".as_ptr()), OK);
            assert_eq!(j.queue(&mut s, b"b\0".as_ptr()), E_BUSY);
            let mut k = Job::new();
            assert_eq!(k.queue(&mut s, b"\0".as_ptr()), dkrt_sdk::proto::E_MSG);
            let mut v = [b'x'; 70].to_vec();
            v.push(0);
            assert_eq!(k.queue(&mut s, v.as_ptr()), dkrt_sdk::proto::E_MSG);
            // no loaded module came from the path: the reply says so at once, and no job is queued
            s.found = None;
            assert_eq!(k.queue(&mut s, b"mods/x.dkm\0".as_ptr()), E_NOT_FOUND);
            assert_eq!(k.phase, 0);
            // the file of a service is no module
            s.found = Some((2, 5));
            s.service = true;
            assert_eq!(k.queue(&mut s, b"services/fx.dks\0".as_ptr()), E_FORMAT);
            assert_eq!(k.phase, 0);
            s.service = false;
            // a finished job takes the next one
            while j.phase == 1 {
                j.poll(&mut s);
            }
            assert_eq!(j.queue(&mut s, b"c\0".as_ptr()), OK);
        }
    }
}
