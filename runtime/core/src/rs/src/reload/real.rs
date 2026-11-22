//! The live job of the device: its static, the commands that queue it and `RealOps`, the job's steps over the loader and the resident.
use super::*;
use dkrt_sdk::*;

/// The live job of the device.
static mut JOB: Job = Job::new();

pub(super) fn job() -> &'static mut Job {
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
        // the service-specific refusals come from `can_stop` of the manager (each service's `teardown_refusal` for the module)
        refusal_for(&crate::PLATFORM, rec, crate::api::card_busy(), |r| crate::lifecycle::get().and_then(|rt| rt.refusal(r)))
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
        crate::state::menus_held(rec)
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
        crate::boot::account_load(k, path, r, crate::loader::missing(), &crate::loader::last_name());
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
