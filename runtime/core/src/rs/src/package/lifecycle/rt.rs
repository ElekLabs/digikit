use super::reload_state::ReloadState;
use super::{RtHost, RtInstallers, RtLinker, RtManifest, RtPlacer, Session};
use super::codes::code_of;
use crate::package::dsp;
use crate::package::handlers::ServiceTable;
use crate::package::loader::{Images, Loader, PhaseKind, Registry};
use crate::package::admission::Admission;
use dkrt_abi::package::PackageId;
use dkrt_abi::plan::{Reason, Verdict};
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::MAX_MODULES;
use super::RtTrace;
use dkrt_lifecycle::{LifecycleError, PackageManager, Refusal, Report, Traced};
use dkrt_sdk::sel::Record;
use core::mem::MaybeUninit;
use core::ptr::addr_of_mut;
use dkrt_sdk::*;

/// The UI passes (`PackageManager::step`) an attach may stay pending before its package is taken down: 6000 passes, a minute or more, since a payload that takes over a DSP range
/// waits for the link.
const ATTACH_PASSES: u32 = 6000;

/// The package manager of the runtime over its seams.
pub type Manager<P, M, R> = PackageManager<
    RtManifest<P, M, R>,
    Traced<RtPlacer<P, M, R>, RtTrace<P, M, R>>,
    Traced<RtLinker<P, M, R>, RtTrace<P, M, R>>,
    Traced<RtInstallers<P, M, R>, RtTrace<P, M, R>>,
    Traced<RtHost<P, M, R>, RtTrace<P, M, R>>,
>;

/// The runtime's package lifecycle: the session the seams share and the manager over them. It lives for the whole run: the boot brings the selection up through it, the live job loads,
/// unloads and reloads modules through it, and the DSP loader reports a failed DSP part to it. It must not move once `init` has run (the seams point into the session).
pub struct Rt<P: PackagePlatform, M: Images, R: Registry> {
    pub(super) session: Session<P, M, R>,
    /// Written in place by `init`; valid while `up`.
    manager: MaybeUninit<Manager<P, M, R>>,
    up: bool,
    /// The take-down of a DSP part whose restore answered busy (a task below the UI task is not parked): the DSP load stays pending and a later UI pass finishes it.
    dsp_down: DspDown,
    /// Where the reload of a service stands (`Rt::reload_closure`): whether the live job must ask again (`dsp_step` then leaves the manager's pending take-down to it) and whether a
    /// member went down.
    pub(super) reload: ReloadState,
}

/// A DSP part's take-down that waits for a busy restore (`Rt::take_down_dsp`): the record, its boot index, the failure code, the passes spent, and whether the load in flight
/// (`in_flight`) or a queued one (`!in_flight`) is the one taken down. `live` is false when nothing waits.
#[derive(Clone, Copy)]
struct DspDown {
    live: bool,
    in_flight: bool,
    rec: usize,
    idx: u32,
    code: i32,
    tries: u32,
}

impl DspDown {
    const NONE: Self = Self { live: false, in_flight: false, rec: 0, idx: 0, code: 0, tries: 0 };
}

/// How a take-down of a DSP part ended: `Done` with whether every record's memory is back, or `Busy` (the manager keeps the rest pending; `Rt::dsp_step` finishes it).
enum Down {
    Done(bool),
    Busy,
}

impl<P: PackagePlatform, M: Images, R: Registry> Rt<P, M, R> {
    /// The lifecycle over `loader` at `out`, with no manager yet, written in place: `Rt` is 8.8 KB and the boot's stack is shared with the firmware, so it is never built on the
    /// stack and moved.
    ///
    /// # Safety
    /// `out` is valid for writes of an `Rt` and stays where it is (the seams point into it).
    pub unsafe fn init_at(out: *mut Self, loader: *mut Loader<P, M, R>) {
        Session::init(addr_of_mut!((*out).session), loader);
        addr_of_mut!((*out).up).write(false);
        addr_of_mut!((*out).dsp_down).write(DspDown::NONE);
        addr_of_mut!((*out).reload).write(ReloadState::Idle);
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
        let manifest = RtManifest::of(&mut self.session, modules);
        let (placer, linker, installers, host) = super::traced_seams(&mut self.session);
        self.up = false;
        PackageManager::init(self.manager.as_mut_ptr(), manifest, placer, linker, installers, host, ATTACH_PASSES)?;
        self.up = true;
        Ok(())
    }

    /// The manager, as a reference that is not tied to a borrow of `self` (like `loader`): one call at a time on one task.
    pub(super) unsafe fn mgr<'a>(&self) -> Option<&'a mut Manager<P, M, R>> {
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
    pub(super) unsafe fn loader<'a>(&self) -> &'a mut Loader<P, M, R> {
        &mut *self.session.loader
    }

    /// The boot's load: every module of the selection is a root of `PackageManager::boot`. The manager orders the services and modules by the package graph, asks the rules of
    /// the core about each package (`Session::admit`), brings the services they need up first, then each module; a package it refuses is reported with the code the loader gave before
    /// (`code_of`), and the rest goes on. The boot record, the log and the module states are written as the boot's load loop wrote them: the providers (log only), then every module
    /// in selection order.
    ///
    /// # Safety
    /// As for `init`; the boot hook's task, once per boot, after `read_tables` and `loader::find_providers`.
    #[inline(never)]
    pub unsafe fn boot(&mut self, rec: &Record) {
        let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
        let n = crate::package::loader::boot_caps(&mut caps);
        self.boot_with(rec, &caps[..n]);
    }

    /// `boot` against the capacity `caps` (`loader::boot_caps`): the host tests give the capacity of the device they boot.
    ///
    /// # Safety
    /// As for `boot`.
    #[inline(never)]
    pub unsafe fn boot_with(&mut self, rec: &Record, caps: &[DkrtCap]) {
        let count = (rec.count as usize).min(SEL_MAX);
        let mut paths = [None; MAX_MODULES];
        for k in 0..count.min(MAX_MODULES) {
            paths[k] = Some(rec.paths[k].as_ptr());
        }
        let op = self.next_op();
        self.enter_op(op);
        let init = self.init(&paths, count);
        self.session.begin_boot(caps, count);
        for k in 0..count {
            crate::package::resident::boot::note_boot_path(k, rec.paths[k].as_ptr());
        }
        let mut roots = [PackageId(0); dkrt_abi::REC_MAX];
        let mut n = 0;
        for k in 0..count.min(MAX_MODULES) {
            roots[n] = PackageId(k as u8);
            n += 1;
        }
        let l = self.loader();
        // a provider whose table the shared parser does not accept is in no graph edge, so nothing would ask for it: it is tried as a root, so that its file is refused with the
        // loader's own code and logged, as the boot always did
        for j in 0..l.provider_count() {
            let id = PackageId((MAX_MODULES + j) as u8);
            if !l.package_table(id).is_some_and(|t| dkneeds::keys::Keys::parse(t).is_ok()) {
                roots[n] = id;
                n += 1;
            }
        }
        l.set_phase(PhaseKind::Boot);
        let report = match (init, self.mgr()) {
            (Ok(()), Some(m)) => m.boot(&roots[..n]),
            (Err(e), _) => Err(e),
            (Ok(()), None) => Err(LifecycleError::NothingPending),
        };
        l.set_phase(PhaseKind::Closed);
        // a boot that did not get as far as its first module still numbers the keys of the modules (none stay)
        if !self.session.numbered {
            match self.mgr() {
                Some(m) => self.session.number_keys(m.graph(), m.live()),
                None => {
                    self.session.numbered = true;
                    l.number_boot_keys(&[false; SEL_MAX], count);
                }
            }
        }
        self.report_boot(rec, report);
        crate::package::boot_idx::set(u32::MAX);
        self.leave_op();
    }

    /// Number a new operation (a boot or a live job): its events carry the number once it is entered.
    pub unsafe fn next_op(&mut self) -> u32 {
        self.loader().next_op()
    }

    /// Operation `op` runs now: the events of the manager's seams and of the installer's gate carry its number until `leave_op`.
    pub unsafe fn enter_op(&mut self, op: u32) {
        self.loader().enter_op(op);
    }

    /// The running operation ended.
    pub unsafe fn leave_op(&mut self) {
        self.loader().leave_op();
    }

    /// The name hash of the service package `service` provides (0 when it provides none).
    unsafe fn provided_name(&self, service: PackageId) -> u32 {
        self.mgr().and_then(|m| m.graph().node(service).and_then(|n| n.provides())).map_or(0, |a| a.name())
    }

    /// The verdict the module states show for the package `id` the manager refused for `why`.
    unsafe fn verdict_of(&mut self, id: PackageId, why: &LifecycleError) -> Verdict {
        let service = if let LifecycleError::NeedRefused { service, .. } = *why { service } else { id };
        verdict_of(why, self.session.reason(id), self.provided_name(service))
    }

    /// The boot record and the log from what the manager did.
    #[inline(never)]
    unsafe fn report_boot(&mut self, rec: &Record, report: Result<Report, LifecycleError>) {
        let count = (rec.count as usize).min(SEL_MAX);
        let empty = Report::default();
        let (report, failed) = match &report {
            Ok(r) => (r, None),
            Err(e) => (&empty, Some(*e)),
        };
        let up = |id: PackageId| report.brought_up().as_slice().contains(&id);
        // a package whose DSP part or attach is not over is LOADING: the end of either is reported later (`dsp_step`)
        let dsp_part = |id: PackageId| report.dsp_pending().as_slice().contains(&id) || report.attaching().as_slice().contains(&id);
        let refusal = |id: PackageId| report.refused().iter().find(|r| r.package == id).map(|r| r.reason);
        let l = self.loader();
        // the providers: log only, in the order the manager tried them (a provider nothing needed was never tried)
        let tried = report.brought_up().as_slice().iter().copied().chain(report.refused().iter().map(|r| r.package));
        for id in tried {
            let Some(j) = (id.0 as usize).checked_sub(MAX_MODULES) else { continue };
            let res = if let Some(why) = refusal(id) {
                code_of(&why)
            } else if dsp_part(id) {
                LOADING
            } else {
                OK
            };
            let o = self.session.outcome[id.0 as usize];
            let ok = res == OK || res == LOADING;
            if !ok {
                // refused at the boot: the live check must not count on it (a provider only taken down since is another matter)
                l.mark_provider_unusable(j);
            }
            let (hash, base) = if res == E_DECL_KIND && o.decl_diff.0 != 0 {
                (o.decl_diff.1, o.decl_diff.0)
            } else if res == E_IMPORT && o.import_hash != 0 {
                (o.import_hash, 0)
            } else if let Some(LifecycleError::NeedRefused { service, .. }) = refusal(id) {
                // refused because a need failed: the line names that need (the root cause), not the service's own hash
                (self.provided_name(service), 0)
            } else {
                (l.provided_hash(j), if ok { crate::package::loader::records::module(id.0 as usize).base } else { 0 })
            };
            crate::package::resident::boot::report_provider(l.provider_file(j), res, hash, base);
        }
        // the modules: the verdict of the graph and the rules of the core, then the load, in selection order
        let mut verdicts = [Verdict::Accepted; SEL_MAX];
        for k in 0..count {
            if let Some(why) = refusal(PackageId(k as u8)) {
                verdicts[k] = self.verdict_of(PackageId(k as u8), &why);
            }
            crate::package::resident::boot::states().planned(k, verdicts[k]);
        }
        for k in 0..count {
            let id = PackageId(k as u8);
            let res = if let Verdict::Refused(why) = verdicts[k] {
                why.code()
            } else {
                crate::package::resident::boot::states().init(k);
                let res = if let Some(e) = failed {
                    code_of(&e)
                } else if up(id) {
                    if dsp_part(id) {
                        LOADING
                    } else {
                        OK
                    }
                } else if let Some(why) = refusal(id) {
                    code_of(&why)
                } else {
                    E_NOT_FOUND
                };
                crate::package::resident::boot::states().finished(k, res);
                res
            };
            let missing = if res == E_IMPORT { self.session.outcome[k].missing } else { [0u8; NEEDS_LEN] };
            crate::package::resident::boot::report_module(k, rec.paths[k].as_ptr(), res, &missing, &l.captured_name(k));
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
            match m.take_down_closure(id) {
                // the consumers that went with the module are no longer active in the boot record (the job marks the module itself)
                Ok(report) => {
                    self.settle_down(id, &report);
                }
                Err(e) => return code_of(&e),
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
        let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
        let n = crate::package::loader::live_capacity(&mut caps, (0, 0), crate::package::loader::dkrt_capacity_into);
        self.live_load_with(k, path, &caps[..n])
    }

    /// `live_load` against the live capacity `caps` (`loader::live_capacity`): the host tests give the capacity of the device they boot.
    pub unsafe fn live_load_with(&mut self, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        let r = self.loader().live_prepare(k, path);
        if r != OK {
            return r;
        }
        let r = self.load_prepared(k, path, caps);
        if r != OK && r != LOADING {
            // the acceptance of the index goes and no free record keeps it
            self.loader().retire_index(k);
        }
        r
    }

    /// Load the new file of a reload into record `rec`, which `unload` took down (`Loader::reload_prepare`, then `PackageManager::load`).
    pub unsafe fn reload_load(&mut self, rec: usize, path: *const u8) -> i32 {
        let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
        let n = crate::package::loader::live_capacity(&mut caps, (0, 0), crate::package::loader::dkrt_capacity_into);
        self.reload_load_with(rec, path, &caps[..n])
    }

    /// `reload_load` against the live capacity `caps` (the old module's own blocks are given back already: it was taken down before).
    pub unsafe fn reload_load_with(&mut self, rec: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        let r = self.loader().reload_prepare(rec, path);
        if r != OK {
            return r;
        }
        self.load_prepared(rec, path, caps)
    }

    unsafe fn load_prepared(&mut self, rec: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        let id = PackageId(rec as u8);
        self.session.modules[rec] = path;
        self.session.outcome[rec] = super::session::Outcome::NONE;
        let l = self.loader();
        l.clear_failure_detail();
        let Some(m) = self.mgr() else {
            return E_NOT_FOUND;
        };
        // the rules of the core judge the module against the live set: the capacity as the live check counts it, and every live module has taken its share
        self.session.admission = Admission::new(caps);
        l.seed_live(&mut self.session.admission, rec);
        self.session.reasons[rec] = None;
        self.session.admitted[rec] = false;
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
                if report.dsp_pending().as_slice().contains(&id) || report.attaching().as_slice().contains(&id) {
                    LOADING
                } else {
                    OK
                }
            }
            Err(e) => code_of(&e),
        }
    }

    /// The DSP part of record `rec` failed or was given up: its closure goes down through the manager (the queued DSP loads of its consumers are removed first), the modules that
    /// went with it are marked taken down (`E_MISSING`). `Done` says whether every record's memory is back (else the DSP state stays retained); `Busy` when the restore waits for
    /// a task to park: the manager keeps the take-down pending and the caller retries it on a later pass.
    unsafe fn take_down_dsp(&mut self, rec: usize) -> Down {
        let id = PackageId(rec as u8);
        let Some(m) = self.mgr() else {
            return Down::Done(false);
        };
        let order = m.graph().reload_order(id, m.live());
        for &d in order.as_slice() {
            if d != id {
                dsp::dequeue(d.0 as usize);
            }
        }
        match m.take_down_closure(id) {
            Ok(report) => Down::Done(self.settle_down(id, &report)),
            Err(LifecycleError::NotLive { .. }) => Down::Done(!self.session.retained(id)),
            // `RestorePending`: an earlier take-down (a queued load given up) still waits for a task to park: this one waits with it
            Err(LifecycleError::Busy { .. } | LifecycleError::RestorePending { .. }) => Down::Busy,
            Err(_) => Down::Done(false),
        }
    }

    /// The modules `report` took down are marked taken down (`E_MISSING`) in the boot record; whether every one's memory is back.
    unsafe fn settle_down(&mut self, id: PackageId, report: &Report) -> bool {
        let mut reclaimed = true;
        for &d in report.taken_down().as_slice() {
            reclaimed &= !self.session.retained(d);
            if d != id && (d.0 as usize) < MAX_MODULES {
                let k = self.session.loader.as_ref().map_or(u8::MAX, |l| l.sel_index(d.0 as usize));
                if k != u8::MAX {
                    crate::package::resident::boot::report_taken_down(k as u32, E_MISSING);
                }
            }
        }
        reclaimed
    }

    /// One UI pass of what waits in the manager: the attaches that answered `Pending` (`PackageManager::step`). The end of each is reported to the boot record the way the end of a
    /// DSP part is: a module that was LOADING is ACTIVE, or FAILED with the code of the refusal, and the modules that went down with a failed package are marked taken down.
    pub(super) unsafe fn step_attaches(&mut self) {
        let Some(m) = self.mgr() else {
            return;
        };
        let report = m.step();
        for &p in report.brought_up().as_slice() {
            if let Some(idx) = self.boot_index(p) {
                crate::package::resident::boot::report_dsp(idx, OK);
            }
        }
        for r in report.refused() {
            let code = code_of(&r.reason);
            self.settle_down(r.package, &report);
            if let Some(idx) = self.boot_index(r.package) {
                crate::package::resident::boot::report_dsp(idx, code);
            } else if let Some(j) = (r.package.0 as usize).checked_sub(MAX_MODULES) {
                let l = self.loader();
                crate::package::resident::boot::report_provider(l.provider_file(j), code, l.provided_hash(j), 0);
            }
        }
    }

    /// The selection index of package `p` when it is a module that has one (the boot record's index).
    unsafe fn boot_index(&self, p: PackageId) -> Option<u32> {
        if (p.0 as usize) >= MAX_MODULES {
            return None;
        }
        let k = self.session.loader.as_ref().map_or(u8::MAX, |l| l.sel_index(p.0 as usize));
        (k != u8::MAX).then_some(k as u32)
    }

    /// The take-down of a DSP part waits for a busy restore: remember it, the DSP load stays pending.
    fn wait_down(&mut self, rec: usize, idx: u32, code: i32, in_flight: bool) {
        self.dsp_down = DspDown { live: true, in_flight, rec, idx, code, tries: 0 };
    }

    /// The end of a DSP part's take-down: the boot record gets the code and the DSP loader goes on (or stays retained when memory is not back).
    unsafe fn end_down(&mut self, d: DspDown, reclaimed: bool) {
        crate::package::resident::boot::report_dsp(d.idx, d.code);
        if d.in_flight {
            dsp::fail_done(reclaimed, d.code);
        } else if !reclaimed {
            dsp::ds().phase = dsp::PH_RETAINED;
        }
    }

    /// Give up the DSP part of module record `rec`: the transfer in flight is taken down and the queue goes on, or the queued load is removed, exactly as when the DSP loader itself
    /// fails. A restore that waits for a task to park keeps the load pending: a later pass finishes it. `false` when `rec` has neither a load in flight nor one queued (its DSP part
    /// has ended: the caller reads the result).
    pub unsafe fn cancel_dsp(&mut self, rec: usize, code: i32) -> bool {
        let d = dsp::ds();
        if d.phase != dsp::PH_IDLE && d.pending.rec as usize == rec {
            let idx = d.pending.boot_idx;
            match self.take_down_dsp(rec) {
                Down::Done(reclaimed) => {
                    crate::package::resident::boot::report_dsp(idx, code);
                    dsp::fail_done(reclaimed, code);
                }
                Down::Busy => self.wait_down(rec, idx, code, true),
            }
            true
        } else if dsp::dequeue(rec) {
            let idx = self.session.loader.as_ref().map_or(u8::MAX, |l| l.sel_index(rec)) as u32;
            match self.take_down_dsp(rec) {
                Down::Done(reclaimed) => self.end_down(DspDown { live: true, in_flight: false, rec, idx, code, tries: 0 }, reclaimed),
                Down::Busy => self.wait_down(rec, idx, code, false),
            }
            true
        } else if let Some(m) = self.mgr().filter(|m| m.state_of(PackageId(rec as u8)) == Some(dkrt_lifecycle::State::Attaching)) {
            // not a DSP load: an attach that stays pending (a payload) is given up the same way
            let id = PackageId(rec as u8);
            if let Ok(report) = m.cancel_attach(id, LifecycleError::Failed { package: id, entering: dkrt_lifecycle::State::Active, status: code }) {
                self.settle_down(id, &report);
                if let Some(idx) = self.boot_index(id) {
                    crate::package::resident::boot::report_dsp(idx, code);
                }
            }
            true
        } else {
            false
        }
    }

    /// One UI-loop pass of the DSP loader. A take-down that waits for a busy restore goes on first (`retry_pending`): when it ends the DSP part's failure is settled, and while a
    /// failed load in flight waits for it the loader does not step. After `GUARD_TICKS` passes the failure is final and the DSP state stays retained. A DSP part that failed takes its
    /// package's closure down through the manager (`take_down_dsp`) before the loader tells the boot record and starts the next load. After a permanent failure or the `GUARD_TICKS`
    /// timeout `dsp_down` is cleared while the manager keeps its pending take-down and retries it silently (acceptable: the retained state ends the session).
    pub unsafe fn dsp_step(&mut self, l: ServiceTable) {
        self.step_attaches();
        if let Some(m) = self.mgr() {
            if m.restore_pending().is_some() && !self.reload.is_waiting() {
                let r = m.retry_pending();
                if self.dsp_down.live {
                    let mut w = self.dsp_down;
                    match r {
                        Ok(report) => {
                            self.dsp_down = DspDown::NONE;
                            let reclaimed = self.settle_down(PackageId(w.rec as u8), &report);
                            self.end_down(w, reclaimed);
                        }
                        Err(LifecycleError::Busy { .. }) if w.tries < dsp::GUARD_TICKS => {
                            w.tries += 1;
                            self.dsp_down = w;
                        }
                        Err(_) => {
                            self.dsp_down = DspDown::NONE;
                            self.end_down(w, false);
                        }
                    }
                }
            }
        }
        if self.dsp_down.live && self.dsp_down.in_flight {
            return;
        }
        match self.loader().dsp_advance(l) {
            dsp::Advance::Pending => {}
            dsp::Advance::Failed(code) => {
                let (rec, idx) = {
                    let p = &dsp::ds().pending;
                    (p.rec as usize, p.boot_idx)
                };
                match self.take_down_dsp(rec) {
                    Down::Done(reclaimed) => {
                        crate::package::resident::boot::report_dsp(idx, code);
                        dsp::fail_done(reclaimed, code);
                    }
                    Down::Busy => self.wait_down(rec, idx, code, true),
                }
            }
            dsp::Advance::Loaded => self.loader().dsp_loaded(l),
        }
    }
}

/// The verdict the module states show for a package the manager refused for `why`: the planner's reasons for a refusal that is the graph's or the rules of the core's, none for a
/// failure of a step (its code is its result). `admission` is why the rules of the core refused the package, `service` the name hash of the service a need refused.
pub(crate) fn verdict_of(why: &LifecycleError, admission: Option<Reason>, service: u32) -> Verdict {
    // if-let chains, not a `match`: a dense match becomes a PC-relative jump table, which is not ColdFire code
    if let LifecycleError::Graph(Refusal::Unmet { service, .. }) = *why {
        Verdict::Refused(Reason::NeedMissing(service))
    } else if let LifecycleError::Graph(Refusal::Incompatible { .. }) = *why {
        Verdict::Refused(Reason::NeedVersion)
    } else if let LifecycleError::Graph(Refusal::Cycle { .. }) = *why {
        Verdict::Refused(Reason::NeedMissing(0))
    } else if let LifecycleError::Graph(Refusal::DuplicateService { service, .. }) = *why {
        Verdict::Refused(Reason::DuplicateOwner(service))
    } else if let LifecycleError::Admission { .. } = *why {
        admission.map_or(Verdict::Accepted, Verdict::Refused)
    } else if let LifecycleError::NeedRefused { .. } = *why {
        Verdict::Refused(Reason::NeedMissing(service))
    } else {
        Verdict::Accepted
    }
}

/// The lifecycle of the device.
mod device {
    use super::*;
    use crate::package::loader::{DeviceImages, RealRegistry};
    use crate::package::resident::platform::TablePlatform;
    use core::mem::MaybeUninit;
    use core::ptr::addr_of_mut;

    pub type Real = Rt<TablePlatform, DeviceImages, RealRegistry>;

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

    /// Start the lifecycle in its place, over `loader`.
    pub unsafe fn start(loader: *mut crate::package::loader::Real) {
        Rt::init_at((*addr_of_mut!(RT)).as_mut_ptr(), loader);
        *addr_of_mut!(UP) = true;
    }

    /// Start the lifecycle in its place and run the boot's load (once per boot).
    pub unsafe fn boot(rec: &Record) {
        start(crate::package::loader::real());
        if let Some(rt) = get() {
            rt.boot(rec);
        }
    }
}

pub(crate) use device::{boot, get};

/// A host test's lifecycle over the real loader (the static of the cleanup tests): a manager over `modules` module packages from the loader's kept tables, with the modules `active`
/// up as if their bring-up had run.
#[cfg(test)]
pub(crate) unsafe fn test_up(modules: usize, active: &[usize]) {
    device::start(crate::package::loader::real());
    let rt = get().unwrap();
    rt.init(&[None; MAX_MODULES], modules).unwrap();
    for &r in active {
        rt.manager().unwrap().assume_active(PackageId(r as u8));
    }
}

/// One UI-loop pass of the DSP loader, through the lifecycle once the boot has started it (a safe boot has no DSP load and starts none).
pub(crate) unsafe fn dsp_step() {
    if let Some(rt) = get() {
        rt.dsp_step(crate::package::handlers::services());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_graphs_refusals_are_the_planners_verdicts() {
        let p = PackageId(1);
        let graph = |r| verdict_of(&LifecycleError::Graph(r), None, 0);
        assert_eq!(graph(Refusal::Unmet { package: p, service: 7 }), Verdict::Refused(Reason::NeedMissing(7)));
        assert_eq!(graph(Refusal::Incompatible { package: p, service: 7 }), Verdict::Refused(Reason::NeedVersion));
        assert_eq!(graph(Refusal::Cycle { package: p }), Verdict::Refused(Reason::NeedMissing(0)));
        // a second provider of one service: the planner's `DuplicateOwner` with the service's hash
        assert_eq!(graph(Refusal::DuplicateService { package: p, service: 7 }), Verdict::Refused(Reason::DuplicateOwner(7)));
        assert_eq!(graph(Refusal::DuplicateService { package: p, service: 7 }).result(), E_PLAN_OWNER);
    }
}
