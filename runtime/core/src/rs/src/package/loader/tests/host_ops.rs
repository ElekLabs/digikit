//! The live job on the host (`reload::Ops`): the job's steps over the loader and the lifecycle the test built and the capacity of its platform. Where the real system's step reads
//! only the runtime's own tables, the real one is called; the DSP is not modelled (`without_dsp` modules), and no card transfer is in flight.

use super::assembly_tests::{live_cap, live_cap_with, Boot, Card, DeviceRegistry};
use crate::fake_platform::FakePlatform;
use crate::package::lifecycle::Rt;
use crate::boot::account_load;
use crate::package::reload::{refusal_for, Ops, RealOps};
use dkrt_sdk::*;

pub(super) struct HostOps<'a> {
    pub(super) l: &'a mut Boot,
    pub(super) rt: &'a mut Rt<FakePlatform, Card, DeviceRegistry>,
}

impl Ops for HostOps<'_> {
    unsafe fn find(&mut self, path: *const u8) -> Option<(usize, usize)> {
        self.l.rec_of_path(path).or_else(|| crate::package::loader::provider_of_path(path).map(|rec| (rec, usize::MAX)))
    }
    unsafe fn is_service(&mut self, path: *const u8) -> bool {
        RealOps.is_service(path)
    }
    unsafe fn alloc_index(&mut self, path: *const u8) -> Result<usize, i32> {
        crate::boot::live_index(path, &self.l.live_at(SEL_MAX))
    }
    unsafe fn refusal(&mut self, rec: Option<usize>) -> Option<i32> {
        let rt = &mut *self.rt;
        match rec {
            Some(r) if r >= dkrt_abi::MAX_MODULES => refusal_for(&self.l.p, None, false, |_| None).or_else(|| rt.closure_refusal(r)),
            _ => refusal_for(&self.l.p, rec, false, |r| rt.refusal(r)),
        }
    }
    unsafe fn check(&mut self, op: u32, rec: usize, k: usize, path: *const u8) -> i32 {
        if op == LIVE_LOAD {
            self.l.live_check_load(k, path, &live_cap(self.l))
        } else if rec >= dkrt_abi::MAX_MODULES {
            self.rt.reload_reset();
            self.l.provider_check(rec, path)
        } else {
            self.l.reload_check(rec, path, &live_cap_with(self.l, crate::package::loader::own_use(rec)))
        }
    }
    fn phase(&mut self, owner: u8) {
        RealOps.phase(owner)
    }
    unsafe fn quiesce(&mut self, rec: usize) -> i32 {
        if rec >= dkrt_abi::MAX_MODULES {
            return self.rt.closure_quiesce(rec);
        }
        self.l.reload_quiesce(rec)
    }
    unsafe fn abort(&mut self, rec: usize) -> bool {
        if rec >= dkrt_abi::MAX_MODULES {
            return self.rt.closure_abort(rec);
        }
        self.l.reload_abort(rec)
    }
    unsafe fn menu_built(&mut self, rec: usize) -> bool {
        if rec >= dkrt_abi::MAX_MODULES {
            return self.rt.closure_menu_built(rec);
        }
        RealOps.menu_built(rec)
    }
    unsafe fn reload_service(&mut self, rec: usize, path: *const u8) -> i32 {
        let mut members = [0usize; crate::package::loader::records::REC_MAX];
        let n = self.rt.closure(rec, &mut members);
        let caps = live_cap_with(self.l, self.l.closure_own(&members[..n]));
        self.rt.reload_closure_with(rec, path, &caps)
    }
    unsafe fn unload(&mut self, rec: usize) -> i32 {
        self.rt.unload(rec)
    }
    unsafe fn dsp_retained(&mut self) -> bool {
        RealOps.dsp_retained()
    }
    unsafe fn dsp_progress(&mut self) -> u32 {
        RealOps.dsp_progress()
    }
    unsafe fn begin_load(&mut self, k: usize) {
        RealOps.begin_load(k)
    }
    unsafe fn load(&mut self, op: u32, rec: usize, k: usize, path: *const u8) -> i32 {
        let caps = live_cap(self.l);
        let r = if op == LIVE_LOAD { self.rt.live_load_with(k, path, &caps) } else { self.rt.reload_load_with(rec, path, &caps) };
        account_load(k, path, r, self.l.missing(), &self.l.last_name);
        r
    }
    unsafe fn unloaded(&mut self, k: usize) {
        self.l.retire_index(k);
        crate::boot::unloaded(k);
    }
    unsafe fn retire(&mut self, k: usize) {
        self.l.retire_index(k);
        crate::boot::restore_live(k);
    }
    unsafe fn dsp_result(&mut self, k: usize) -> Option<i32> {
        RealOps.dsp_result(k)
    }
    unsafe fn cancel_dsp(&mut self, rec: usize, code: i32) -> bool {
        self.rt.cancel_dsp(rec, code)
    }
    unsafe fn on_loaded(&mut self, rec: usize) {
        RealOps.on_loaded(rec)
    }
    unsafe fn finish(&mut self, ok: bool, changed: bool) {
        RealOps.finish(ok, changed)
    }
    fn blocks(&mut self) -> Option<u32> {
        RealOps.blocks()
    }
    unsafe fn next_op(&mut self) -> u32 {
        self.rt.next_op()
    }
    unsafe fn enter_op(&mut self, op: u32) {
        self.rt.enter_op(op)
    }
    unsafe fn leave_op(&mut self) {
        self.rt.leave_op()
    }
}
