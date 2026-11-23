//! What the boot (`boot::run`) asks of the system besides the platform: the steps that read the firmware's RAM, the stores and the intro screen, or reach the loader and the
//! lifecycle of the device. `RealBootEnv` is the device's; a host test gives the same steps over a fake card and its own loader (`loader/tests/host_boot_env.rs`).

use crate::platform::RealPlatform;
use crate::fault;
use crate::level::PackageStatus;
use crate::hooks;
use crate::stage1::{self, ArenaPlacer, Placer, Stage1, StoreSource, PACKAGE_PATH};
use crate::state::DKRT_STAGE;
use core::cell::{Cell, RefCell};
use core::ptr::addr_of_mut;
use dkrt_abi::dev_addr::DevAddr;
use dkrt_abi::platform::Platform;
use dkrt_sdk::sel::Record;

/// The steps of the boot that depend on where it runs. The order and the decisions between them are `boot::run`'s.
pub(crate) trait BootEnv {
    type P: Platform;

    /// The platform of the boot: the selection record, the card and the boot counter go through it. `boot::run` asks for it at each use and keeps no reference across the steps below, which
    /// may borrow the loader that owns the platform on the host.
    fn platform(&self) -> &Self::P;
    /// The settings record into RAM.
    unsafe fn settings_load(&self);
    /// The probe word of the keep area (RAM that survives a restart).
    unsafe fn keep_probe(&self);
    /// The keep area's crash record of the boot before: logged and cleared; true when there was one.
    unsafe fn crash_at_boot(&self) -> bool;
    /// Stage 1: find the system package on the card, place it at the arena top and check that the resident table fits it. What it found is the one input of the level
    /// decision that is not read from the record, the key or the crash records. A package that is not `Ok` is not left in the arena.
    unsafe fn load_package(&self) -> PackageStatus;
    /// Give the placed package the resident table and take its hooks (`stage1::start`). Only after `load_package` gave `Ok`, and only when the level runs the package.
    /// False when the package gave no hooks: the boot then runs core-only.
    unsafe fn start_package(&self) -> bool;
    /// The placed package is not going to run this boot (core-only by crashes, or it gave no hooks): its block goes back to the arena. Harmless when nothing was placed.
    unsafe fn abandon_package(&self);
    /// A safe boot: the installer writes nothing.
    unsafe fn set_safe(&self);
    /// Read the file of each selected module and keep its header table for the boot's graph.
    unsafe fn read_tables(&self, rec: &Record);
    /// Search the card for the providers the selection `count` modules long needs, from the kept tables.
    unsafe fn find_providers(&self, count: usize);
    /// The package manager's load of the selection.
    unsafe fn load(&self, rec: &Record);
}

/// The device's boot: the real platform, the loader and the lifecycle of the image. `placed` is stage 1's result for `start_package`: the entry of the package and the
/// resident table minor it was built against.
pub(crate) struct RealBootEnv {
    placed: Cell<Option<(DevAddr, u16)>>,
    /// The placer of stage 1: it keeps the package's block, so that the boot can give it back (`abandon_package`) after stage 1 is done.
    placer: RefCell<ArenaPlacer>,
}

impl RealBootEnv {
    pub(crate) const fn new() -> RealBootEnv {
        RealBootEnv { placed: Cell::new(None), placer: RefCell::new(ArenaPlacer::new()) }
    }
}

impl BootEnv for RealBootEnv {
    type P = RealPlatform;

    fn platform(&self) -> &RealPlatform {
        &crate::platform::PLATFORM
    }
    unsafe fn settings_load(&self) {
        crate::settings::load()
    }
    unsafe fn keep_probe(&self) {
        fault::keep_probe()
    }
    unsafe fn crash_at_boot(&self) -> bool {
        fault::at_boot()
    }
    unsafe fn load_package(&self) -> PackageStatus {
        let chunk = &mut (*addr_of_mut!(DKRT_STAGE)).0;
        let mut placer = self.placer.borrow_mut();
        let mut stage1 = Stage1::new(StoreSource::new(PACKAGE_PATH), &mut *placer, chunk);
        let status = stage1.load_checked(&crate::resident::TABLE);
        self.placed.set(stage1.placed());
        status
    }
    unsafe fn start_package(&self) -> bool {
        let Some((entry, _)) = self.placed.get() else { return false };
        match stage1::start(entry, &crate::resident::TABLE) {
            Some(hooks) => {
                hooks::install(hooks);
                true
            }
            None => false,
        }
    }
    unsafe fn abandon_package(&self) {
        self.placer.borrow_mut().abandon();
        self.placed.set(None);
    }
    unsafe fn set_safe(&self) {
        hooks::boot_safe()
    }
    unsafe fn read_tables(&self, rec: &Record) {
        hooks::boot_read_tables(rec)
    }
    unsafe fn find_providers(&self, count: usize) {
        hooks::boot_find_providers(count)
    }
    unsafe fn load(&self, rec: &Record) {
        hooks::boot_load(rec)
    }
}

#[cfg(test)]
mod tests;
