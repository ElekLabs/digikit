//! A `BootEnv` over `FakePlatform` for the tests of the boot's own decisions (the level, the boot counter, the stable period): the steps that reach the loader, the settings and the
//! keep area's RAM only record that they ran, and the crash record, the DSP hang record and the package status are knobs a test sets before the boot.
//! The loader's own host environment is `package/loader/tests/host_boot_env.rs`; this one needs no firmware image.

use crate::boot_env::BootEnv;
use crate::fake_platform::FakePlatform;
use crate::level::PackageStatus;
use dkrt_sdk::sel::Record;
use std::cell::{Cell, RefCell};
use std::vec::Vec;

pub(crate) struct FakeBootEnv<'a> {
    p: &'a FakePlatform,
    /// What stage 1 found of the package.
    pub package: Cell<PackageStatus>,
    /// The boot before crashed after its stable period (a crash record with `stable`).
    pub late_crash: Cell<bool>,
    /// The boot before left a DSP hang record / a panic record.
    pub hang: Cell<bool>,
    pub panic: Cell<bool>,
    /// The steps that ran, in order.
    pub steps: RefCell<Vec<&'static str>>,
}

impl<'a> FakeBootEnv<'a> {
    pub(crate) fn new(p: &'a FakePlatform) -> FakeBootEnv<'a> {
        FakeBootEnv {
            p,
            package: Cell::new(PackageStatus::Ok),
            late_crash: Cell::new(false),
            hang: Cell::new(false),
            panic: Cell::new(false),
            steps: RefCell::new(Vec::new()),
        }
    }

    /// Did the package manager's load run?
    pub(crate) fn loaded(&self) -> bool {
        self.steps.borrow().contains(&"load")
    }

    fn note(&self, step: &'static str) {
        self.steps.borrow_mut().push(step);
    }
}

impl BootEnv for FakeBootEnv<'_> {
    type P = FakePlatform;

    fn platform(&self) -> &FakePlatform {
        self.p
    }
    unsafe fn package_status(&self) -> PackageStatus {
        self.package.get()
    }
    unsafe fn settings_load(&self) {
        self.note("settings");
    }
    unsafe fn keep_probe(&self) {
        self.note("probe");
    }
    /// The crash record of the boot before, as `fault::at_boot` leaves it in `DKRT_CRASH`.
    unsafe fn crash_at_boot(&self) -> bool {
        let c = &mut *core::ptr::addr_of_mut!(crate::fault::DKRT_CRASH);
        c.found = self.late_crash.get() as u32;
        c.stable = self.late_crash.get() as u32;
        c.hang = self.hang.get() as u32;
        c.panic = self.panic.get() as u32;
        self.late_crash.get() || self.hang.get() || self.panic.get()
    }
    unsafe fn set_safe(&self) {
        self.note("safe");
    }
    unsafe fn read_tables(&self, _rec: &Record) {
        self.note("tables");
    }
    unsafe fn find_providers(&self, _count: usize) {
        self.note("providers");
    }
    unsafe fn load(&self, _rec: &Record) {
        self.note("load");
    }
}
