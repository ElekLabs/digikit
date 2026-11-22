//! Interface C of docs/ARCHITECTURE.md, the core's side: `RealCore` (what a service calls through `CoreApi`), `FakeCore` (host tests) and the few core functions
//! a service may name directly. The trait and the pure types are in the crate `dkrt-abi`. The services reach the core through this module only
//! (`check:layers` enforces it).
//!
//! `RealCore` is a zero-sized type that delegates to the core functions, so a call through it costs what the direct call cost. A function that a host test must
//! drive takes a generic `C: CoreApi` (no `dyn`: the runtime has no allocator and no vtables); everything else calls the `REAL_CORE` constant.
use crate::{dlog, registry, settings, worker};
use dkrt_abi::link::{LinkResult, Owner, Sequence, Slot, Stream, Ticket};
use dkrt_abi::dsp::DspState;
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

pub use dkrt_abi::core_api::{CoreApi, JOB_E_CANCELLED, LOG_INFO};

// The platform facts a service may read (the device profile, the interrupt mask, the sample-pool provider): the only platform names a service has.
pub use crate::platform::PLATFORM;
pub use dkrt_abi::platform::{Platform, SamplePool};
pub use dkrt_abi::storage::Storage;

/// What the services' host tests use of the core's job queue.
#[cfg(test)]
pub(crate) mod testing {
    pub(crate) use crate::test_lock::LOCK as TEST_LOCK;
    pub(crate) use crate::worker::{cancel, test_pump, test_reset};
}

/// The core's own functions, called through `CoreApi`.
#[derive(Clone, Copy)]
pub struct RealCore;

/// The value a function that needs no host test passes to a `CoreApi` call.
pub const REAL_CORE: RealCore = RealCore;

impl dkrt_abi::mem::FwMem for RealCore {}

impl CoreApi for RealCore {
    fn boot_index(&self) -> u32 {
        unsafe { core::ptr::read_volatile(core::ptr::addr_of!(crate::selection::BOOT_IDX)) }
    }
    fn live_op(&self) -> u32 {
        crate::reload::phase()
    }
    fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        crate::loader::keys_of(at, kind, out)
    }
    fn legacy_state(&self) -> [u8; 32] {
        let map = unsafe { (*core::ptr::addr_of!(crate::selection::REC)).map };
        let mut b = [0u8; 32];
        let mut i = 0;
        while i < map.len() {
            b[4 * i..4 * i + 4].copy_from_slice(&map[i].to_be_bytes());
            i += 1;
        }
        b
    }
    unsafe fn legacy_state_clear(&self) -> i32 {
        use crate::selection::{update, REC, REC_COPY, REC_VALID};
        let mut r = REC;
        r.map = [0; dkrt_sdk::sel::MAP_N];
        update(r, REC_COPY, REC_VALID)
    }
    fn device_id(&self) -> u32 {
        PLATFORM.device_id()
    }
    unsafe fn module_name(&self, owner: u8) -> *const u8 {
        crate::platform::st().modules[owner as usize - 1].name.as_ptr()
    }
    unsafe fn module_of(&self, ctx: *const u8) -> u8 {
        match crate::entry::own_rec(ctx as *mut u8) {
            Some(rec) => (rec + 1) as u8,
            None => 0,
        }
    }
    unsafe fn storage_stat(&self, root: u32, path: &[u8]) -> Result<dkrt_abi::storage::Stat, i32> {
        PLATFORM.stat(root, path)
    }
    unsafe fn storage_stat_head(&self, root: u32, path: &[u8], head: &mut [u8; 64]) -> Result<(dkrt_abi::storage::Stat, usize), i32> {
        PLATFORM.stat_head(root, path, head)
    }
    fn irq_mask(&self) -> u32 {
        PLATFORM.irq_mask()
    }
    fn irq_restore(&self, sr: u32) {
        PLATFORM.irq_restore(sr)
    }
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32 {
        settings::get(key, out)
    }
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32 {
        settings::put(key, value)
    }
    unsafe fn settings_overlay_set(&self, overlay: *const dkrt_abi::service::SettingsOverlay) {
        crate::settings_overlay::set(overlay)
    }
    unsafe fn settings_changed(&self, id: &[u8]) {
        crate::settings_reload::changed(id)
    }
    unsafe fn live(&self, op: u32, path: *const u8) -> i32 {
        crate::reload::dkrt_live(op, path)
    }
    unsafe fn live_state(&self) -> dkrt_sdk::DkrtLive {
        let mut out = core::mem::MaybeUninit::<dkrt_sdk::DkrtLive>::uninit();
        crate::reload::dkrt_live_state(out.as_mut_ptr());
        out.assume_init()
    }
    unsafe fn selection_add(&self, path: &[u8]) -> i32 {
        crate::selection::selection_add(path)
    }
    unsafe fn module_path(&self, owner: u8) -> *const u8 {
        crate::scan::module_path(owner)
    }
    unsafe fn store_find(&self, dir: &[u8], name: &[u8], out: &mut [u8; 64]) -> i32 {
        crate::scan::store_find(dir, name, out)
    }
    unsafe fn log(&self, kind: u16, flags: u16, args: [u32; 5]) {
        dlog::ev(kind, flags, args)
    }
    fn current_source(&self) -> u8 {
        dlog::current_source()
    }
    unsafe fn job_post(&self, owner: u8, step: Option<JobStepFn>, done: Option<JobDoneFn>, ctx: *mut u8) -> i32 {
        worker::post(owner, step, done, ctx)
    }
    unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32 {
        worker::cancel(owner, id)
    }
    fn dsp(&self) -> &'static mut DspState {
        crate::dsp::ds()
    }
    unsafe fn info(&self, name_hash: u32) -> *const u32 {
        PLATFORM.info(name_hash)
    }
    fn link_set(&self, slot: Slot, value: u32) {
        crate::link::lk().set(slot, value)
    }
    fn link_confirmed(&self, slot: Slot) -> Option<u32> {
        crate::link::lk().confirmed(slot)
    }
    fn link_errors(&self) -> u32 {
        crate::link::lk().errors()
    }
    fn link_forget(&self, win: u32, first: u32, count: u32) {
        crate::link::lk().forget(win, first, count)
    }
    unsafe fn link_submit(&self, owner: Owner, stream: &Stream) -> Option<Ticket> {
        crate::link::lk().submit(owner, stream)
    }
    fn link_result(&self, t: Ticket) -> LinkResult {
        crate::link::lk().result(t)
    }
    unsafe fn link_drain(&self, owner: Owner) -> bool {
        crate::link::lk().drain(owner)
    }
    unsafe fn link_run(&self, owner: Owner, seq: &Sequence) -> bool {
        crate::link::lk().run(owner, seq)
    }
}

#[cfg(test)]
pub use crate::fake_platform::FakePlatform;
#[cfg(test)]
pub use dkrt_test_support::{Call, FakeCore};

/// The shared lock of the tests that drive the runtime statics, for tests inside services.
#[cfg(test)]
pub(crate) use crate::test_lock::LOCK as TEST_LOCK;
