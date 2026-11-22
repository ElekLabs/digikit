//! `CheckedImages`: the images of the loader static in host tests. It is `RealImages` and checks one thing, the order of a take-down: the module block of a cleanup test goes back
//! to the arena only after the DSP side let go of it (`mod_len` is cleared first). The cleanup tests name the block with `watch`.

use dkrt_abi::module::Ctx;
use crate::loader::{Block, Images, RealImages, Staged};
use dkrt_abi::dev_addr::DevAddr;
use std::cell::Cell;

std::thread_local! {
    /// The module block of the cleanup test of this thread (`NULL`: none).
    static WATCHED: Cell<DevAddr> = const { Cell::new(DevAddr::NULL) };
}

/// Check the free of the block `block` from now on (`DevAddr::NULL`: check none).
pub(crate) fn watch(block: DevAddr) {
    WATCHED.with(|w| w.set(block));
}

pub struct CheckedImages;

impl Images for CheckedImages {
    unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32> {
        RealImages.stage(path)
    }
    unsafe fn alloc(&mut self, n: u32) -> Option<Block> {
        RealImages.alloc(n)
    }
    unsafe fn free(&mut self, addr: DevAddr) {
        if !addr.is_null() && addr == WATCHED.with(|w| w.get()) {
            assert_eq!(crate::dsp::ds().mod_len[0], 0, "the module block is freed after its DSP length is cleared");
        }
        RealImages.free(addr)
    }
    unsafe fn import_addr(&mut self, name: *const u8) -> u32 {
        RealImages.import_addr(name)
    }
    unsafe fn svc_import_addr(&mut self, name: *const u8) -> u32 {
        RealImages.svc_import_addr(name)
    }
    unsafe fn import_needs(&mut self, name: *const u8) -> Option<&'static str> {
        RealImages.import_needs(name)
    }
    unsafe fn list(&mut self, prefix: *const u8, out: *mut dkrt_sdk::DkrtStoreEntry, max: u32) -> i32 {
        RealImages.list(prefix, out, max)
    }
    unsafe fn enter_init(&mut self, rec: usize, init: u32, ctx: *const Ctx) -> i32 {
        RealImages.enter_init(rec, init, ctx)
    }
    unsafe fn enter_fini(&mut self, rec: usize, fini: u32, ctx: *const Ctx) {
        RealImages.enter_fini(rec, fini, ctx)
    }
}
