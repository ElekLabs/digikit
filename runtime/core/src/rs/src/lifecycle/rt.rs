use super::{RtHost, RtInstallers, RtLinker, RtManifest, RtPlacer, Session};
use super::codes::code_of;
use crate::dsp;
use crate::handlers::ServiceTable;
use crate::loader::{Images, Loader, PhaseKind, Registry};
use crate::plan::Plan;
use dkrt_abi::package::PackageId;
use dkrt_abi::plan::Verdict;
use dkrt_abi::platform::Platform;
use dkrt_abi::MAX_MODULES;
use dkrt_lifecycle::{LifecycleError, PackageManager, Report};
use dkrt_sdk::sel::Record;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use dkrt_sdk::*;

/// The package manager of the runtime over its seams.
pub type Manager<P, M, R> = PackageManager<RtManifest<P, M, R>, RtPlacer<P, M, R>, RtLinker<P, M, R>, RtInstallers<P, M, R>, RtHost<P, M, R>>;

/// The runtime's package lifecycle: the session the seams share and the manager over them. It lives for the whole run: the boot brings the selection up through it, the live job loads,
/// unloads and reloads modules through it, and the DSP loader reports a failed DSP part to it. It must not move once `init` has run (the seams point into the session).
pub struct Rt<P: Platform, M: Images, R: Registry> {
    session: Session<P, M, R>,
    /// Written in place by `init`; valid while `up`.
    manager: MaybeUninit<Manager<P, M, R>>,
    up: bool,
}

impl<P: Platform, M: Images, R: Registry> Rt<P, M, R> {
    /// The lifecycle over `loader` at `out`, with no manager yet, written in place: `Rt` is 8.8 KB and the boot's stack is shared with the firmware, so it is never built on the
    /// stack and moved.
    ///
    /// # Safety
    /// `out` is valid for writes of an `Rt` and stays where it is (the seams point into it).
    pub unsafe fn init_at(out: *mut Self, loader: *mut Loader<P, M, R>) {
        Session::init(addr_of_mut!((*out).session), loader);
        addr_of_mut!((*out).up).write(false);
    }

    /// The lifecycle over `loader`, by value (host tests; nothing is started yet, so it may move until `boot`).
    #[cfg(test)]
    pub fn new(loader: *mut Loader<P, M, R>) -> Self {
        let mut at = MaybeUninit::<Self>::uninit();
        unsafe {
            Self::init_at(at.as_mut_ptr(), loader);
            at.assume_init()
        }
    }

    /// Build the manager over the loader's kept tables, in its place in `self`: the module at each selection index (`paths`, its store path) and the providers of the search. Call
    /// it once, when `self` is in its place for good.
    ///
    /// # Safety
    /// The loader outlives `self`, its tables do not change under the manifest's views except through `Rt`, and the paths stay valid while their module is up.
    pub unsafe fn init(&mut self, paths: &[Option<*const u8>; MAX_MODULES], modules: usize) -> Result<(), LifecycleError> {
        for (k, p) in paths.iter().enumerate() {
            self.session.modules[k] = p.unwrap_or(core::ptr::null());
        }
        let manifest = RtManifest::of(&self.session, modules);
        let (placer, linker, installers, host) = self.session.seams();
        self.up = false;
        PackageManager::init(self.manager.as_mut_ptr(), manifest, placer, linker, installers, host)?;
        self.up = true;
        Ok(())
    }

    /// The manager, as a reference that is not tied to a borrow of `self` (like `loader`): one call at a time on one task.
    unsafe fn mgr<'a>(&self) -> Option<&'a mut Manager<P, M, R>> {
        if self.up {
            Some(&mut *(self.manager.as_ptr() as *mut Manager<P, M, R>))
        } else {
            None
        }
    }

    pub fn manager(&mut self) -> Option<&mut Manager<P, M, R>> {
        unsafe { self.mgr() }
    }

    pub fn session(&self) -> &Session<P, M, R> {
        &self.session
    }

    /// The loader. Not tied to a borrow of `self`: the manager and the session reach it through their own pointers, one call at a time on one task.
    unsafe fn loader<'a>(&self) -> &'a mut Loader<P, M, R> {
        &mut *self.session.loader
    }

    /// The boot's load: the modules `planned` accepted are the roots of `PackageManager::boot`. The manager brings the services they need up first, then each module; a
    /// package it refuses is reported with the code the loader gave before (`code_of`), and the rest goes on. The boot record, the log and the module states are written
    /// as the boot's load loop wrote them: the providers (log only), then every module in plan order.
    ///
    /// # Safety
    /// As for `init`; the boot hook's task, once per boot, after `read_tables` and the plan.
    #[inline(never)]
    pub unsafe fn boot(&mut self, rec: &Record, planned: Plan) {
        let count = (rec.count as usize).min(SEL_MAX);
        let mut paths = [None; MAX_MODULES];
        for k in 0..count.min(MAX_MODULES) {
            paths[k] = Some(rec.paths[k].as_ptr());
        }
        let init = self.init(&paths, count);
        self.session.plan = planned;
        self.session.plan_count = count;
        self.session.settled = false;
        for j in 0..self.session.plan.np {
            if let Verdict::Refused(why) = self.session.plan.provider_verdicts[j] {
                self.session.provider_refusal[j] = why.code();
            }
        }
        for k in 0..count {
            crate::boot::note_boot_path(k, rec.paths[k].as_ptr());
        }
        let mut roots = [PackageId(0); SEL_MAX];
        let mut n = 0;
        for k in 0..count.min(MAX_MODULES) {
            if self.session.plan.verdicts[k].is_accepted() {
                roots[n] = PackageId(k as u8);
                n += 1;
            }
        }
        let l = self.loader();
        l.set_phase(PhaseKind::Boot);
        let report = match (init, self.mgr()) {
            (Ok(()), Some(m)) => m.boot(&roots[..n]),
            (Err(e), _) => Err(e),
            (Ok(()), None) => Err(LifecycleError::NothingPending),
        };
        l.set_phase(PhaseKind::Closed);
        self.session.settle_once();
        self.report_boot(rec, report);
        // the plan's verdicts and the refusals of its services are the boot's: a live load decides again from the services as they are then
        self.session.plan_count = 0;
        self.session.provider_refusal = [0; PROVIDER_MAX];
        crate::selection::BOOT_IDX = u32::MAX;
    }

    /// The boot record and the log from what the manager did and the plan after its settle.
    #[inline(never)]
    unsafe fn report_boot(&mut self, rec: &Record, report: Result<Report, LifecycleError>) {
        let count = (rec.count as usize).min(SEL_MAX);
        let empty = Report::default();
        let (report, failed) = match &report {
            Ok(r) => (r, None),
            Err(e) => (&empty, Some(*e)),
        };
        let up = |id: PackageId| report.brought_up().as_slice().contains(&id);
        let dsp_part = |id: PackageId| report.dsp_pending().as_slice().contains(&id);
        let l = self.loader();
        // the providers: log only
        let mut pos = 0;
        while pos < self.session.plan.np {
            let j = self.session.plan.provider_order[pos] as usize;
            pos += 1;
            let id = PackageId((MAX_MODULES + j) as u8);
            let refused = report.refused().iter().find(|r| r.package == id).map(|r| code_of(&r.reason));
            let res = if self.session.provider_refusal[j] != 0 {
                self.session.provider_refusal[j]
            } else if let Some(code) = refused {
                code
            } else if up(id) {
                if dsp_part(id) {
                    LOADING
                } else {
                    OK
                }
            } else {
                continue; // nothing needed it: it was never tried
            };
            let o = self.session.outcome[id.0 as usize];
            let ok = res == OK || res == LOADING;
            if !ok {
                // refused at the boot: the live plan must not count on it (a provider only taken down since is another matter)
                l.mark_provider_unusable(j);
            }
            let (hash, base) = if res == E_DECL_KIND && o.decl_diff.0 != 0 {
                (o.decl_diff.1, o.decl_diff.0)
            } else if res == E_IMPORT && o.import_hash != 0 {
                (o.import_hash, 0)
            } else {
                (l.provided_hash(j), if ok { crate::loader::records::module(id.0 as usize).base } else { 0 })
            };
            crate::boot::report_provider(l.provider_file(j), res, hash, base);
        }
        // the modules: the plan's verdict after settle, then the load in plan order
        for k in 0..count {
            crate::boot::states().planned(k, self.session.plan.verdicts[k]);
        }
        let mut pos = 0;
        while pos < count {
            let k = self.session.plan.order[pos] as usize;
            pos += 1;
            let id = PackageId(k as u8);
            let res = match self.session.plan.verdicts[k] {
                Verdict::Refused(why) => why.code(),
                Verdict::Accepted => {
                    crate::boot::states().init(k);
                    let res = if let Some(e) = failed {
                        code_of(&e)
                    } else if up(id) {
                        if dsp_part(id) {
                            LOADING
                        } else {
                            OK
                        }
                    } else if let Some(r) = report.refused().iter().find(|r| r.package == id) {
                        code_of(&r.reason)
                    } else {
                        E_NOT_FOUND
                    };
                    crate::boot::states().finished(k, res);
                    res
                }
            };
            let missing = if res == E_IMPORT { self.session.outcome[k].missing } else { [0u8; NEEDS_LEN] };
            crate::boot::report_module(k, rec.paths[k].as_ptr(), res, &missing, &l.captured_name(k));
        }
    }

    /// Whether the module at record `rec` may be taken down now: the refusals of its services (`can_stop` for it) and of the services that need it. `None` when nothing refuses.
    pub unsafe fn refusal(&mut self, rec: usize) -> Option<i32> {
        let m = self.mgr()?;
        match m.check_stop(PackageId(rec as u8)) {
            Ok(()) => None,
            Err(LifecycleError::NotLive { .. }) => None,
            Err(e) => Some(code_of(&e)),
        }
    }

    /// Take module record `rec` down for the live job: its live consumers and the module itself, no service (`take_down_closure`). The steps that can answer busy run first, while the
    /// module is whole, so that the job can give its tick back; a record whose memory could not be released is tried again on the next call. `OK`, or `E_BUSY` while the
    /// job must ask again.
    pub unsafe fn unload(&mut self, rec: usize) -> i32 {
        let id = PackageId(rec as u8);
        let Some(m) = self.mgr() else {
            return E_NOT_FOUND;
        };
        if m.live().contains(id) {
            if !self.session.loader.as_mut().is_some_and(|l| l.package_retire_ready(id)) {
                return E_BUSY;
            }
            if let Err(e) = m.take_down_closure(id) {
                return code_of(&e);
            }
        } else if self.session.retained(id) {
            self.session.retry_free(id);
        }
        if self.session.retained(id) {
            E_BUSY
        } else {
            OK
        }
    }

    /// Load the file at `path` into selection index `k` as module record `k` (`Loader::live_prepare`, then `PackageManager::load`). `OK`, `LOADING` (the DSP part follows), or the
    /// refusal in the loader's codes.
    pub unsafe fn live_load(&mut self, k: usize, path: *const u8) -> i32 {
        let r = self.loader().live_prepare(k, path);
        if r != OK {
            return r;
        }
        let r = self.load_prepared(k, path);
        if r != OK && r != LOADING {
            // as `Loader::live_load` did: the plan's acceptance goes and no free record keeps the index
            self.loader().retire_index(k);
        }
        r
    }

    /// Load the new file of a reload into record `rec`, which `unload` took down (`Loader::reload_prepare`, then `PackageManager::load`).
    pub unsafe fn reload_load(&mut self, rec: usize, path: *const u8) -> i32 {
        let r = self.loader().reload_prepare(rec, path);
        if r != OK {
            return r;
        }
        self.load_prepared(rec, path)
    }

    unsafe fn load_prepared(&mut self, rec: usize, path: *const u8) -> i32 {
        let id = PackageId(rec as u8);
        self.session.modules[rec] = path;
        self.session.outcome[rec] = super::session::Outcome::NONE;
        let l = self.loader();
        l.clear_failure_detail();
        let Some(m) = self.mgr() else {
            return E_NOT_FOUND;
        };
        // the file's table and the needs it names are the ones the graph must hold
        m.manifest_mut().reread(SEL_MAX);
        if let Err(e) = m.refresh_graph() {
            return code_of(&e);
        }
        l.set_phase(PhaseKind::Live);
        let out = m.load(id);
        l.set_phase(PhaseKind::Closed);
        match out {
            Ok(report) => {
                if report.dsp_pending().as_slice().contains(&id) {
                    LOADING
                } else {
                    OK
                }
            }
            Err(e) => code_of(&e),
        }
    }

    /// The DSP part of record `rec` failed or was given up: its closure goes down through the manager (the queued DSP loads of its consumers are removed first), the modules that
    /// went with it are marked taken down (`E_MISSING`). Whether every record's memory is back (else the DSP state stays retained).
    unsafe fn take_down_dsp(&mut self, rec: usize) -> bool {
        let id = PackageId(rec as u8);
        let Some(m) = self.mgr() else {
            return false;
        };
        let order = m.graph().reload_order(id, m.live());
        for &d in order.as_slice() {
            if d != id {
                dsp::dequeue(d.0 as usize);
            }
        }
        match m.take_down_closure(id) {
            Ok(report) => {
                let mut reclaimed = true;
                for &d in report.taken_down().as_slice() {
                    reclaimed &= !self.session.retained(d);
                    if d != id && (d.0 as usize) < MAX_MODULES {
                        let k = self.session.loader.as_ref().map_or(u8::MAX, |l| l.sel_index(d.0 as usize));
                        if k != u8::MAX {
                            crate::boot::report_taken_down(k as u32, E_MISSING);
                        }
                    }
                }
                reclaimed
            }
            Err(LifecycleError::NotLive { .. }) => !self.session.retained(id),
            Err(_) => false,
        }
    }

    /// Give up the DSP part of module record `rec`: the transfer in flight is taken down and the queue goes on, or the queued load is removed, exactly as when the DSP loader itself
    /// fails. `false` when `rec` has neither a load in flight nor one queued (its DSP part has ended: the caller reads the result).
    pub unsafe fn cancel_dsp(&mut self, rec: usize, code: i32) -> bool {
        let d = dsp::ds();
        if d.phase != dsp::PH_IDLE && d.pending.rec as usize == rec {
            let idx = d.pending.boot_idx;
            let reclaimed = self.take_down_dsp(rec);
            crate::boot::report_dsp(idx, code);
            dsp::fail_done(reclaimed, code);
            true
        } else if dsp::dequeue(rec) {
            let idx = self.session.loader.as_ref().map_or(u8::MAX, |l| l.sel_index(rec)) as u32;
            if !self.take_down_dsp(rec) {
                dsp::ds().phase = dsp::PH_RETAINED;
            }
            crate::boot::report_dsp(idx, code);
            true
        } else {
            false
        }
    }

    /// One UI-loop pass of the DSP loader. A take-down that waits for a busy restore goes on first. A DSP part that failed takes its package's closure down through the
    /// manager (`take_down_dsp`) before the loader tells the boot record and starts the next load.
    pub unsafe fn dsp_step(&mut self, l: ServiceTable) {
        if let Some(m) = self.mgr() {
            if m.restore_pending().is_some() {
                let _ = m.retry_pending();
            }
        }
        match self.loader().dsp_advance(l) {
            dsp::Advance::Pending => {}
            dsp::Advance::Failed(code) => {
                let (rec, idx) = {
                    let p = &dsp::ds().pending;
                    (p.rec as usize, p.boot_idx)
                };
                let reclaimed = self.take_down_dsp(rec);
                crate::boot::report_dsp(idx, code);
                dsp::fail_done(reclaimed, code);
            }
            dsp::Advance::Loaded => self.loader().dsp_loaded(l),
        }
    }
}

/// The lifecycle of the device.
mod device {
    use super::*;
    use crate::loader::{RealImages, RealRegistry};
    use crate::platform::RealPlatform;
    use core::mem::MaybeUninit;
    use core::ptr::addr_of_mut;

    pub type Real = Rt<RealPlatform, RealImages, RealRegistry>;

    /// Zero bytes until the boot starts it (a `MaybeUninit`, so the static stays out of `.data`).
    static mut RT: MaybeUninit<Real> = MaybeUninit::uninit();
    static mut UP: bool = false;

    pub fn get() -> Option<&'static mut Real> {
        unsafe {
            if *addr_of_mut!(UP) {
                Some(&mut *(*addr_of_mut!(RT)).as_mut_ptr())
            } else {
                None
            }
        }
    }

    /// Start the lifecycle in its place and run the boot's load (once per boot).
    pub unsafe fn boot(rec: &Record, planned: Plan) {
        Rt::init_at((*addr_of_mut!(RT)).as_mut_ptr(), crate::loader::real());
        *addr_of_mut!(UP) = true;
        if let Some(rt) = get() {
            rt.boot(rec, planned);
        }
    }
}

pub(crate) use device::{boot, get};

/// One UI-loop pass of the DSP loader: through the lifecycle once the boot has started it, else the loader's own step (a safe boot has no DSP load).
pub(crate) unsafe fn dsp_step() {
    match get() {
        Some(rt) => rt.dsp_step(crate::handlers::services()),
        None => crate::loader::dsp_step(),
    }
}
