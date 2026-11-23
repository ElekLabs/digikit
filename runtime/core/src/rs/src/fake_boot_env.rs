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
    /// The arena block the placed package lives in; `None`: no block (a placement the host directory cannot describe).
    pub extent: Cell<Option<(u32, u32)>>,
    /// The host directory as the package finds it when it is started, before it fills its own entries: `(ready, core-owned ids filled)`; `None` before it started.
    pub dir_at_start: Cell<Option<(bool, usize)>>,
    /// The package gives no hooks when started.
    pub start_fails: Cell<bool>,
    /// The boot counter the card held when the package started (what a fault in its entry leaves behind); `None` before it started or without a record.
    pub counter_at_start: Cell<Option<u32>>,
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
            extent: Cell::new(Some((0x4010_0000, 0x4011_0000))),
            dir_at_start: Cell::new(None),
            start_fails: Cell::new(false),
            counter_at_start: Cell::new(None),
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
    unsafe fn load_package(&self) -> PackageStatus {
        self.note("stage1");
        self.package.get()
    }
    unsafe fn package_extent(&self) -> Option<(u32, u32)> {
        self.extent.get()
    }
    /// The test-only attach: the package of this binary gets the resident table (the device's stage 1 places the package file instead).
    unsafe fn start_package(&self) -> bool {
        self.note("package");
        let held = match crate::selection::current_with(self.p) {
            crate::selection::Choice::Use { record, .. } => Some(record.boot_counter),
            _ => None,
        };
        self.counter_at_start.set(held);
        let dir = *core::ptr::addr_of!(crate::host_dir::DKRT_HOST_DIR);
        self.dir_at_start.set(Some((dir.is_ready(), dkrt_sdk::HOST_CORE_OWNED.iter().filter(|&&id| dir.get(id).is_some()).count())));
        if self.start_fails.get() {
            return false;
        }
        crate::hooks::attach_package(&crate::resident::TABLE);
        true
    }
    unsafe fn abandon_package(&self) {
        self.note("abandon");
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
