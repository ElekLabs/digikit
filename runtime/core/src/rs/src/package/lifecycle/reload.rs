//! The reload of a service and its consumers through the manager (`Rt::reload_closure`): the live job's one step for a service file. The manager takes the consumers and the
//! service down, the service comes up from the new file and the consumers come back (`PackageManager::reload`). The new file was checked first (`Loader::provider_check`), the
//! services number the keys again before the first consumer starts (`ProviderHost::services_up`), and nothing here has a rollback: a failure after the take-down leaves the
//! closure down (the job reports it as torn).
use super::codes::code_of;
use super::rt::Rt;
use crate::package::admission::Admission;
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

    /// Has the reload of a service taken a member down (the live job reports it as torn)? Meaningful after `reload_closure` answered anything but `OK`.
    pub fn reload_torn(&self) -> bool {
        self.reload_torn
    }

    /// The live job gives up waiting for a busy restore: the manager keeps its pending take-down and `dsp_step` retries it silently, as after a failed DSP part.
    pub fn reload_abandon(&mut self) {
        self.reloading = false;
    }

    /// Reload service record `rec` from the file at `path` with its consumers (`Loader::provider_prepare`, then `PackageManager::reload`), called once per UI pass by the live job.
    /// `OK`; `E_BUSY` while a restore waits for a task to park (nothing is lost, the job asks again and the same call finishes it); or the refusal in the loader's codes. A refusal
    /// before the take-down leaves everything as it was (`reload_torn` false); after it the closure is down.
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
        if self.reloading {
            let out = m.retry_pending();
            return self.reload_end(out, &[]);
        }
        if rec < MAX_MODULES || !m.live().contains(id) {
            return E_NOT_FOUND;
        }
        self.reload_torn = false;
        let mut members = [0usize; REC_MAX];
        let n = self.closure(rec, &mut members);
        let members = &members[..n];
        let l = self.loader();
        // the old table, for a refusal before anything is taken down
        let mut old = [0u8; TABLE_MAX];
        let old_len = l.provider_table_into(rec, &mut old);
        let r = l.provider_prepare(rec, path);
        if r != OK {
            return r;
        }
        for &r in members {
            // a consumer comes back from the file it came from; the path pointer a live load gave is the job's buffer, which holds this job's path now
            if r < MAX_MODULES {
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
        let down = members.iter().any(|&r| !m.live().contains(PackageId(r as u8))) || m.restore_pending().is_some();
        if down || out.is_ok() {
            return self.reload_end(out, members);
        }
        // refused before the take-down: the old file stays loaded with its own table
        let code = self.reload_end(out, members);
        l.provider_table_restore(rec, &old[..old_len]);
        m.manifest_mut().reread(SEL_MAX);
        let _ = m.refresh_graph();
        code
    }

    /// The end of one call of the reload: the code, and whether a restore keeps the rest pending.
    unsafe fn reload_end(&mut self, out: Result<Report, LifecycleError>, members: &[usize]) -> i32 {
        let Some(m) = self.mgr() else {
            return E_NOT_FOUND;
        };
        match out {
            Ok(_) => {
                self.reloading = false;
                OK
            }
            Err(e) => {
                self.reloading = m.restore_pending().is_some();
                if !members.is_empty() {
                    self.reload_torn = self.reloading || members.iter().any(|&r| !m.live().contains(PackageId(r as u8)));
                }
                code_of(&e)
            }
        }
    }
}
