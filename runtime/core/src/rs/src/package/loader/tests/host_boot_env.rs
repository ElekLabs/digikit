//! The boot's environment on the host (`BootEnv`): the platform is the fake one of the loader, the tables and the load go through the loader and the lifecycle the test built, and the
//! steps the host cannot do (the settings store, the keep area's RAM) do nothing.

use super::assembly_tests::{capacity, Boot};
use crate::boot_env::BootEnv;
use crate::fake_platform::FakePlatform;
use crate::package::lifecycle::Rt;
use dkrt_sdk::sel::Record;

use super::assembly_tests::{Card, DeviceRegistry};

pub(super) struct HostBootEnv {
    l: *mut Boot,
    rt: *mut Rt<FakePlatform, Card, DeviceRegistry>,
}

impl HostBootEnv {
    /// The boot over loader `l` and the lifecycle `rt` made over it; both outlive the env.
    pub(super) fn new(l: &mut Boot, rt: &mut Rt<FakePlatform, Card, DeviceRegistry>) -> HostBootEnv {
        HostBootEnv { l, rt }
    }
}

impl BootEnv for HostBootEnv {
    type P = FakePlatform;

    /// A fresh shared borrow of the loader's platform per call, through the raw pointer: `boot::run` holds none across `set_safe`, `read_tables`, `find_providers`, which make `&mut Boot`.
    fn platform(&self) -> &FakePlatform {
        unsafe { &(*self.l).p }
    }
    /// The package of the host's boot is the one linked into the test: stage 1 is not part of this environment.
    unsafe fn load_package(&self) -> crate::level::PackageStatus {
        crate::level::PackageStatus::Ok
    }
    unsafe fn start_package(&self) -> bool {
        crate::hooks::attach_package(&crate::resident::TABLE);
        true
    }
    unsafe fn abandon_package(&self) {}
    /// The host has no settings store (`FakeStorage` models the files, not the record).
    unsafe fn settings_load(&self) {}
    /// The keep area is firmware RAM at a raw address.
    unsafe fn keep_probe(&self) {}
    unsafe fn crash_at_boot(&self) -> bool {
        false
    }
    unsafe fn set_safe(&self) {
        (*self.l).set_safe()
    }
    /// The files of the card of the test, as the staging buffer would hold them.
    unsafe fn read_tables(&self, rec: &Record) {
        let l = &mut *self.l;
        l.reset_plan();
        for k in 0..rec.count as usize {
            let n = dkrt_abi::mem::cstr_len(rec.paths[k].as_ptr(), 64);
            let file = l.m.files.iter().find(|(name, _)| name[..] == rec.paths[k][..n]).map(|(_, bytes)| bytes.clone());
            if let Some(file) = file {
                l.capture(k, &file);
            }
        }
    }
    unsafe fn find_providers(&self, count: usize) {
        (*self.l).find_providers(count)
    }
    unsafe fn load(&self, rec: &Record) {
        (*self.rt).boot_with(rec, &capacity(&(*self.l).p))
    }
}
