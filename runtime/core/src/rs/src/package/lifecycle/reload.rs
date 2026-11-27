//! The reload of a service and its consumers through the manager (`Rt::reload_closure`): the live job's one step for a service file. The manager takes the consumers and the
//! service down, the service comes up from the new file and the consumers come back (`PackageManager::reload`). The new file was checked first (`Loader::provider_check`), the
//! services number the keys again before the first consumer starts (`ProviderHost::services_up`), and nothing here has a rollback: a failure after the take-down leaves the
//! closure down (the job reports it as torn).
use super::codes::code_of;
use super::service_copy::ServiceCopy;
use super::rt::Rt;
use crate::package::admission::Admission;
use crate::package::loader::records::is_service_rec;
use crate::package::loader::{records, Images, PhaseKind, Registry, TABLE_MAX};
use dkrt_abi::package::PackageId;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::{MAX_MODULES, REC_MAX};
use dkrt_lifecycle::{LifecycleError, Report};
use dkrt_sdk::*;

impl<P: PackagePlatform, M: Images, R: Registry> Rt<P, M, R> {
    /// The records of the closure a reload of record `rec` takes down and brings back: `rec` and its live consumers, consumers first (`reload_order`). Their count.
    pub unsafe fn closure(&self, rec: usize, out: &mut [usize; REC_MAX]) -> usize {
        let Some(m) = self.mgr() else {
            return 0;
        };
        let order = m.graph().reload_order(PackageId(rec as u8), m.live());
        let mut n = 0;
        for p in order.as_slice() {
            if n < REC_MAX {
                out[n] = p.0 as usize;
                n += 1;
            }
        }
        n
    }

    /// Does a member of the closure of record `rec` have a DSP part? A closure reload has no DSP step: the job refuses it.
    pub unsafe fn closure_has_dsp(&self, rec: usize) -> bool {
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        members[..n].iter().any(|&r| crate::package::dsp::ds().mod_len[r] != 0)
    }

    /// Was a menu object built for a module of the closure of record `rec` (it points into the module's code)?
    pub unsafe fn closure_menu_built(&self, rec: usize) -> bool {
        // once the reload has its room the closure may be down: the menus are not asked again
        if self.reload.is_called() {
            return false;
        }
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        members[..n].iter().any(|&r| !is_service_rec(r) && crate::package::resident::state::menus_held(r))
    }

    /// The refusals of a reload of service record `rec` that the system facts do not hold: the menu of a consumer, a DSP part in the closure (`E_BOOT_ONLY`: the closure reload has
    /// no DSP step, a restart reloads it) and the `can_stop` of the closure (`refusal`).
    pub unsafe fn closure_refusal(&mut self, rec: usize) -> Option<i32> {
        if self.closure_menu_built(rec) {
            return Some(E_RELOAD_MENUS);
        }
        if self.closure_has_dsp(rec) {
            return Some(E_BOOT_ONLY);
        }
        self.refusal(rec)
    }

    /// The drain of every member of the closure of record `rec` (`Loader::reload_quiesce`): `E_NOT_FOUND` when a record is gone, `E_BUSY` while one is not drained, else `OK`.
    /// Each member's call is repeatable.
    pub unsafe fn closure_quiesce(&mut self, rec: usize) -> i32 {
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        if n == 0 {
            return E_NOT_FOUND;
        }
        let mut out = OK;
        for &r in &members[..n] {
            let q = self.loader().reload_quiesce(r);
            if q == E_NOT_FOUND {
                return q;
            }
            if q != OK {
                out = q;
            }
        }
        out
    }

    /// Does the session still hold the memory of a member of the closure of record `rec`?
    unsafe fn closure_retained(&mut self, rec: usize) -> bool {
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        let mut any = false;
        for &r in &members[..n] {
            let id = PackageId(r as u8);
            if self.session.retained(id) {
                self.session.retry_free(id);
                any |= self.session.retained(id);
            }
        }
        any
    }

    /// A reload of service record `rec` cannot go on: every member gets its tick back (`Loader::reload_abort`). False when the reload went on (a member is down, or a restore of the
    /// closure waits: it is left to `dsp_step`).
    pub unsafe fn closure_abort(&mut self, rec: usize) -> bool {
        if self.reload.is_torn() || self.reload.is_waiting() {
            let l = self.loader();
            self.reload.stop_waiting(&mut self.session.carried, l);
            return false;
        }
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        for &r in &members[..n] {
            self.loader().reload_abort(r);
        }
        self.session.carried.release_all(self.loader());
        true
    }

    /// Has the reload of a service taken a member down (the live job reports it as torn)? Meaningful after `reload_closure` answered anything but `OK`.
    pub fn reload_torn(&self) -> bool {
        self.reload.is_torn()
    }

    /// A new reload starts: what an earlier one left is forgotten.
    pub fn reload_reset(&mut self) {
        let l = unsafe { self.loader() };
        unsafe { self.reload.reset(&mut self.session.carried, l) };
    }

    /// Reload service record `rec` from the file at `path` with its consumers (`Loader::provider_prepare`, then `PackageManager::reload`), called once per UI pass by the live job.
    /// `OK`; `E_BUSY` while a restore waits for a task to park (nothing is lost, the job asks again and the same call finishes it); or the refusal in the loader's codes. A refusal
    /// before the take-down leaves everything as it was (`reload_torn()` false); after it the closure is down.
    pub unsafe fn reload_closure(&mut self, rec: usize, path: *const u8) -> i32 {
        let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        let own = self.loader().closure_own(&members[..n]);
        let k = crate::package::loader::live_capacity(&mut caps, own, crate::package::loader::dkrt_capacity_into);
        self.reload_closure_with(rec, path, &caps[..k])
    }

    /// `reload_closure` against the live capacity `caps` (the members' own blocks counted as given back): the host tests give the capacity of the device they boot.
    pub unsafe fn reload_closure_with(&mut self, rec: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        let id = PackageId(rec as u8);
        let Some(m) = self.mgr() else {
            return E_NOT_FOUND;
        };
        if self.reload.is_waiting() {
            if m.restore_pending().is_some() {
                let out = m.retry_pending();
                return self.reload_end(out, rec, &[]);
            }
            // the manager is done: the old copies' memory may still be held (a file or I/O step in flight)
            if self.closure_retained(rec) {
                return E_BUSY;
            }
            return self.reload_end(Ok(Report::default()), rec, &[]);
        }
        if !is_service_rec(rec) || !m.live().contains(id) {
            return E_NOT_FOUND;
        }
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        let members = &members[..n];
        // as a module unload: a member whose file or I/O step still runs, or whose memory is held, is not taken down (nothing has changed)
        for &r in members {
            if !self.loader().package_retire_ready(PackageId(r as u8)) {
                return E_BUSY;
            }
        }
        let l = self.loader();
        // the old table, for a refusal before anything is taken down
        let mut old = [0u8; TABLE_MAX];
        let old_len = l.provider_table_into(rec, &mut old);
        let r = l.provider_prepare(rec, path);
        if r != OK {
            return r;
        }
        // the state the old copies hand over gets its room before anything is taken down: a full arena is a refusal, as it is for any other check
        let carry = self.session.carried.reserve(members, ServiceCopy::of, l);
        if carry != OK {
            l.provider_table_restore(rec, &old[..old_len]);
            return carry;
        }
        self.reload.call();
        let mut live = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
        let k = caps.len().min(CAP_MAX);
        live[..k].copy_from_slice(&caps[..k]);
        // the blocks stay taken until the new copies are up: they are not part of what the reload gives back
        hold_in(&mut live[..k], self.session.carried.held_bytes());
        let caps = &live[..k];
        for &r in members {
            // a consumer comes back from the file it came from; the path pointer a live load gave is the job's buffer, which holds this job's path now
            if !is_service_rec(r) {
                self.session.modules[r] = records::path(r).as_ptr();
            }
            self.session.outcome[r] = super::session::Outcome::NONE;
            self.session.reasons[r] = None;
            self.session.admitted[r] = false;
        }
        self.session.admission = Admission::new(caps);
        l.seed_live_without(&mut self.session.admission, members);
        l.clear_failure_detail();
        m.manifest_mut().reread(SEL_MAX);
        let out = match m.refresh_graph() {
            Ok(()) => {
                l.set_phase(PhaseKind::Live);
                let out = m.reload(id);
                l.set_phase(PhaseKind::Closed);
                out
            }
            Err(e) => Err(e),
        };
        if m.restore_pending().is_none() {
            // the new copies took what they take (or the reload ended before the take-down): nothing waits for the bytes
            self.session.carried.release_all(l);
        }
        let down = members.iter().any(|&r| !m.live().contains(PackageId(r as u8))) || m.restore_pending().is_some();
        if out.is_ok() && !down && self.closure_retained(rec) {
            // up again, but an old copy's memory is not back yet: the job asks again
            self.reload.wait_retained();
            return E_BUSY;
        }
        if down || out.is_ok() {
            return self.reload_end(out, rec, members);
        }
        // refused before the take-down: the old file stays loaded with its own table
        let code = self.reload_end(out, rec, members);
        l.provider_table_restore(rec, &old[..old_len]);
        m.manifest_mut().reread(SEL_MAX);
        let _ = m.refresh_graph();
        code
    }

    /// The reload failed and the restore is not pending: the members that are not back are down for good. The boot record says what is true for each: a consumer that was ACTIVE is
    /// FAILED with `code` (`report_taken_down`, the late take-down's record), and the service's failure goes to the log like a provider that did not load.
    unsafe fn report_closure_down(&mut self, members: &[usize], code: i32) {
        let Some(m) = self.mgr() else {
            return;
        };
        for &r in members {
            if m.live().contains(PackageId(r as u8)) {
                continue;
            }
            let l = self.loader();
            if let Some(j) = r.checked_sub(MAX_MODULES) {
                crate::package::resident::boot::report_provider(l.provider_file(j), code, l.provided_hash(j), 0);
            } else {
                let k = l.sel_index(r);
                if k != u8::MAX {
                    crate::package::resident::boot::report_taken_down(k as u32, code);
                }
            }
        }
    }

    /// The end of one call of the reload: the code, and whether a restore keeps the rest pending.
    unsafe fn reload_end(&mut self, out: Result<Report, LifecycleError>, root: usize, members: &[usize]) -> i32 {
        let Some(m) = self.mgr() else {
            return E_NOT_FOUND;
        };
        if m.restore_pending().is_none() {
            self.session.carried.release_all(self.loader());
        }
        match out {
            Ok(_) => {
                let l = self.loader();
                self.reload.done(&mut self.session.carried, l);
                // every module that is back hears it (a module of the closure has no DSP part: the job refused one)
                let mut now = [0usize; REC_MAX];
                let n = self.closure(root, &mut now);
                for &r in &now[..n] {
                    if !is_service_rec(r) {
                        crate::package::handlers::services().on_loaded(r);
                    }
                }
                OK
            }
            Err(e) => {
                let down = (!members.is_empty()).then(|| members.iter().any(|&r| !m.live().contains(PackageId(r as u8))));
                let l = self.loader();
                self.reload.failed(m.restore_pending().is_some(), down, &mut self.session.carried, l);
                let code = code_of(&e);
                if self.reload.is_torn() && !self.reload.is_waiting() {
                    self.report_closure_down(members, code);
                }
                code
            }
        }
    }
}

/// `caps` with `held` more arena bytes in use (the state blocks of a reload): the arena's `used` and its largest free block.
fn hold_in(caps: &mut [DkrtCap], held: u32) {
    for c in caps.iter_mut() {
        if c.kind == dkrt_sdk::proto::CAPK_ARENA as u32 {
            c.used = c.used.saturating_add(held);
            c.largest = c.largest.saturating_sub(held);
        }
    }
}
