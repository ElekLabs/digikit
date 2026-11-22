//! What the boot (`boot::run`) asks of the system besides the platform: the steps that read the firmware's RAM, the stores and the intro screen, or reach the loader and the
//! lifecycle of the device. `RealBootEnv` is the device's; a host test gives the same steps over a fake card and its own loader (`loader/tests/host_boot_env.rs`).

use crate::platform::RealPlatform;
use crate::{fault};
use crate::package::{loader};
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
    /// A safe boot: the installer writes nothing.
    unsafe fn set_safe(&self);
    /// Read the file of each selected module and keep its header table for the boot's graph.
    unsafe fn read_tables(&self, rec: &Record);
    /// Search the card for the providers the selection `count` modules long needs, from the kept tables.
    unsafe fn find_providers(&self, count: usize);
    /// The package manager's load of the selection.
    unsafe fn load(&self, rec: &Record);
}

/// The device's boot: the real platform, the loader and the lifecycle of the image.
pub(crate) struct RealBootEnv;

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
    unsafe fn set_safe(&self) {
        loader::set_safe()
    }
    unsafe fn read_tables(&self, rec: &Record) {
        crate::package::boot::read_tables(rec)
    }
    unsafe fn find_providers(&self, count: usize) {
        crate::package::boot::find_providers(count)
    }
    unsafe fn load(&self, rec: &Record) {
        crate::package::boot::load(rec)
    }
}
