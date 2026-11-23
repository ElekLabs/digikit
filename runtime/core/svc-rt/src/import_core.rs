//! `ImportCore`: a zero-size `CoreApi` over the `dkc_*` imports of `dkrt_abi::core_imports`, and `sys`, the `extern "C"` block of those names.
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::DspState;
use dkrt_abi::link::{LinkResult, Owner, Sequence, Slot, Stream, Ticket};
use dkrt_sdk::api::{JobDoneFn, JobStepFn};

/// The imports (`dkrt_abi::core_imports`): the names the runtime's `SVC_IMPORTS` resolves when the file is loaded.
pub mod sys {
    macro_rules! declare {
        ($(fn $name:ident($($arg:ident: $ty:ty),* $(,)?) $(-> $ret:ty)?;)*) => {
            extern "C" {
                $(pub fn $name($($arg: $ty),*) $(-> $ret)?;)*
            }
        };
    }
    dkrt_abi::core_imports!(declare);
}

use sys::*;

/// `CoreApi` over the `dkc_*` imports.
#[derive(Clone, Copy)]
pub struct ImportCore;

impl dkrt_abi::mem::FwMem for ImportCore {}

impl CoreApi for ImportCore {
    fn boot_index(&self) -> u32 {
        unsafe { dkc_boot_index() }
    }
    fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        unsafe { dkc_keys_of(at, kind, out.as_mut_ptr()) as usize }
    }
    fn legacy_state(&self) -> [u8; 32] {
        let mut b = [0u8; 32];
        unsafe { dkc_legacy_state(b.as_mut_ptr()) };
        b
    }
    unsafe fn legacy_state_clear(&self) -> i32 {
        dkc_legacy_state_clear()
    }
    fn live_op(&self) -> u32 {
        unsafe { dkc_live_op() }
    }
    fn device_id(&self) -> u32 {
        unsafe { dkc_device_id() }
    }
    unsafe fn module_name(&self, owner: u8) -> *const u8 {
        dkc_module_name(owner)
    }
    unsafe fn module_of(&self, ctx: *const u8) -> u8 {
        dkc_module_of(ctx)
    }
    unsafe fn storage_stat(&self, root: u32, path: &[u8]) -> Result<dkrt_abi::storage::Stat, i32> {
        let mut w = dkrt_abi::storage::StatWire::empty();
        let r = dkc_storage_stat(root, path.as_ptr(), path.len() as u32, &mut w);
        if r < 0 {
            return Err(r);
        }
        Ok(w.stat())
    }
    unsafe fn storage_stat_head(
        &self,
        root: u32,
        path: &[u8],
        head: &mut [u8; 64],
    ) -> Result<(dkrt_abi::storage::Stat, usize), i32> {
        let mut w = dkrt_abi::storage::StatWire::empty();
        let r = dkc_storage_stat_head(
            root,
            path.as_ptr(),
            path.len() as u32,
            &mut w,
            head.as_mut_ptr(),
        );
        if r < 0 {
            return Err(r);
        }
        Ok((w.stat(), r as usize))
    }
    fn irq_mask(&self) -> u32 {
        #[cfg(target_arch = "m68k")]
        {
            unsafe { dkc_irq_mask() }
        }
        // a host build links no runtime: nothing preempts a host test, so masking is nothing (the unit tests of `Owner` count masks and deliver interrupts at them)
        #[cfg(all(not(target_arch = "m68k"), test))]
        {
            crate::owner::test_irq_mask()
        }
        #[cfg(all(not(target_arch = "m68k"), not(test)))]
        {
            0
        }
    }
    fn irq_restore(&self, sr: u32) {
        #[cfg(target_arch = "m68k")]
        {
            unsafe { dkc_irq_restore(sr) }
        }
        #[cfg(all(not(target_arch = "m68k"), test))]
        {
            crate::owner::test_irq_restore(sr)
        }
        #[cfg(all(not(target_arch = "m68k"), not(test)))]
        {
            let _ = sr;
        }
    }
    unsafe fn settings_get(&self, key: &[u8], out: &mut [u8]) -> i32 {
        dkc_settings_get(
            key.as_ptr(),
            key.len() as u32,
            out.as_mut_ptr(),
            out.len() as u32,
        )
    }
    unsafe fn settings_put(&self, key: &[u8], value: &[u8]) -> i32 {
        dkc_settings_put(
            key.as_ptr(),
            key.len() as u32,
            value.as_ptr(),
            value.len() as u32,
        )
    }
    unsafe fn settings_overlay_set(&self, overlay: *const dkrt_abi::service::SettingsOverlay) {
        dkc_settings_overlay_set(overlay)
    }
    unsafe fn settings_changed(&self, id: &[u8]) {
        dkc_settings_changed(id.as_ptr(), id.len() as u32)
    }
    unsafe fn reload_when_possible(&self, id: &[u8]) {
        dkc_reload_when_possible(id.as_ptr(), id.len() as u32)
    }
    unsafe fn live(&self, op: u32, path: *const u8) -> i32 {
        dkc_live(op, path)
    }
    unsafe fn live_state(&self) -> dkrt_sdk::DkrtLive {
        let mut out = core::mem::MaybeUninit::<dkrt_sdk::DkrtLive>::uninit();
        dkc_live_state(out.as_mut_ptr());
        out.assume_init()
    }
    unsafe fn selection_add(&self, path: &[u8]) -> i32 {
        dkc_selection_add(path.as_ptr(), path.len() as u32)
    }
    unsafe fn module_path(&self, owner: u8) -> *const u8 {
        dkc_module_path(owner)
    }
    unsafe fn store_find(&self, dir: &[u8], name: &[u8], out: &mut [u8; 64]) -> i32 {
        dkc_store_find(
            dir.as_ptr(),
            dir.len() as u32,
            name.as_ptr(),
            name.len() as u32,
            out.as_mut_ptr(),
        )
    }
    unsafe fn log(&self, kind: u16, flags: u16, args: [u32; 5]) {
        dkc_log(kind, flags, args.as_ptr())
    }
    fn current_source(&self) -> u8 {
        unsafe { dkc_current_source() }
    }
    unsafe fn job_post(
        &self,
        owner: u8,
        step: Option<JobStepFn>,
        done: Option<JobDoneFn>,
        ctx: *mut u8,
    ) -> i32 {
        dkc_job_post(owner, step, done, ctx)
    }
    unsafe fn job_cancel(&self, owner: u8, id: u32) -> i32 {
        dkc_job_cancel(owner, id)
    }
    fn dsp(&self) -> &'static mut DspState {
        unsafe { &mut *dkc_dsp() }
    }
    unsafe fn info(&self, name_hash: u32) -> *const u32 {
        dkc_info(name_hash)
    }
    fn link_set(&self, slot: Slot, value: u32) {
        unsafe { dkc_link_set(&slot, value) }
    }
    fn link_confirmed(&self, slot: Slot) -> Option<u32> {
        let mut v = 0u32;
        if unsafe { dkc_link_confirmed(&slot, &mut v) } {
            Some(v)
        } else {
            None
        }
    }
    fn link_errors(&self) -> u32 {
        unsafe { dkc_link_errors() }
    }
    fn link_forget(&self, win: u32, first: u32, count: u32) {
        unsafe { dkc_link_forget(win, first, count) }
    }
    unsafe fn link_submit(&self, owner: Owner, stream: &Stream) -> Option<Ticket> {
        let mut t = core::mem::MaybeUninit::<Ticket>::uninit();
        if dkc_link_submit(owner, stream, t.as_mut_ptr()) {
            Some(t.assume_init())
        } else {
            None
        }
    }
    fn link_result(&self, t: Ticket) -> LinkResult {
        unsafe { dkc_link_result(&t) }
    }
    unsafe fn link_drain(&self, owner: Owner) -> bool {
        dkc_link_drain(owner)
    }
    unsafe fn link_run(&self, owner: Owner, seq: &Sequence) -> bool {
        dkc_link_run(owner, seq)
    }
}

/// Run `f` with interrupts masked (a single core: nothing else runs meanwhile).
#[inline(always)]
pub(crate) fn masked<R>(f: impl FnOnce() -> R) -> R {
    let sr = ImportCore.irq_mask();
    let r = f();
    ImportCore.irq_restore(sr);
    r
}
