use super::session::Session;
use crate::package::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::package_platform::PackagePlatform;
use dkrt_abi::provider::Status;
use dkrt_abi::REC_MAX;
use dkrt_fit::ledger::{ResourceDecl, Want};
use dkrt_lifecycle::{Api, Entry, Manifest};

/// The header tables of the selected packages and of the services they need, as the loader keeps them: the module at each selection index (`Loader::capture`), the
/// provider the search found (`discover::find`). The tables stay where the loader holds them; the manifest holds views of them, so the loader's tables must not change while
/// the manager lives. The core's APIs are the runtime floor and the services of the image (`HANDLE_SERVICES`, the in-crate rows), all at the runtime's version.
///
/// Today's headers pack no pools and no claims, so the manifest has none (RC3 packs them). `admit` is the rules of the core (`admission.rs`) over the loader's tables. The entries of a package are those its start derived (`Session`), and are valid
/// from the start of that package to the start of the next, which is when the manager installs them.
pub struct RtManifest<P: PackagePlatform, M: Images, R: Registry> {
    shared: *mut Session<P, M, R>,
    views: [(PackageId, &'static [u8]); REC_MAX],
    n: usize,
    core: CoreApis,
}

/// The table of a module that has none the shared parser accepts (a file that is missing, torn or of the old format): no needs, nothing provided. Such a module is still a node of the
/// graph, so the manager places it and the loader's own checks refuse it with the code they always gave.
fn empty_table() -> &'static [u8] {
    static mut EMPTY: [u8; 64] = [0; 64];
    static mut LEN: usize = 0;
    unsafe {
        let buf = &mut *core::ptr::addr_of_mut!(EMPTY);
        if *core::ptr::addr_of!(LEN) == 0 {
            *core::ptr::addr_of_mut!(LEN) = dkneeds::keys::write_table(&dkneeds::keys::Table::default(), buf).unwrap_or(0);
        }
        &buf[..*core::ptr::addr_of!(LEN)]
    }
}

/// The most APIs of the core: the names of `HANDLE_SERVICES` and of the in-crate rows.
const CORE_MAX: usize = 16;

/// The APIs the core provides without a package: the services of the image (`HANDLE_SERVICES`, `runtime` among them) and the in-crate rows, all at the runtime's version.
pub struct CoreApis {
    apis: [Api; CORE_MAX],
    n: usize,
}

impl CoreApis {
    pub fn new() -> CoreApis {
        let mut c = CoreApis { apis: [Api::new(0, 0); CORE_MAX], n: 0 };
        let version = dkrt_sdk::RUNTIME_VERSION;
        for name in dkrt_sdk::records::HANDLE_SERVICES {
            c.apis[c.n] = Api::new(dkneeds::keys::name_hash(name), version);
            c.n += 1;
        }
        for row in &crate::package::svc_rows::rows()[..crate::package::gen_services::STATIC] {
            c.apis[c.n] = Api::new(unsafe { row.name_hash() }, version);
            c.n += 1;
        }
        c
    }

    pub fn as_slice(&self) -> &[Api] {
        &self.apis[..self.n]
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> RtManifest<P, M, R> {
    /// The manifest of the first `modules` selection indices and the providers of the search, over the loader of `shared`.
    ///
    /// # Safety
    /// `shared` and its loader outlive the manifest, and the loader's kept tables do not change while it lives.
    pub unsafe fn of(shared: *mut Session<P, M, R>, modules: usize) -> Self {
        let mut m = RtManifest { shared, views: [(PackageId(0), &[]); REC_MAX], n: 0, core: CoreApis::new() };
        m.reread(modules);
        m
    }

    /// Read the loader's kept tables again (a file was added or replaced since): the first `modules` selection indices and the providers of the search. A table that the shared
    /// parser does not accept (or none) is the empty table, for a module and for a provider alike, so that one bad file does not refuse the whole graph: its package is a node that
    /// needs nothing and provides nothing, so it is accepted by the graph and the loader's own checks refuse its file; it never meets a need.
    ///
    /// # Safety
    /// As for [`RtManifest::of`].
    pub unsafe fn reread(&mut self, modules: usize) {
        let l = &*(*self.shared).loader;
        let empty = empty_table();
        let mut n = 0;
        let mut add = |id: usize| {
            let table = match l.package_table(PackageId(id as u8)) {
                Some(t) if dkneeds::keys::Keys::parse(t).is_ok() => t,
                _ => empty,
            };
            // a view of the loader's own array (or of the empty table), which outlives the manifest
            self.views[n] = (PackageId(id as u8), core::slice::from_raw_parts(table.as_ptr(), table.len()));
            n += 1;
        };
        for k in 0..modules.min(dkrt_abi::MAX_MODULES) {
            add(k);
        }
        for j in 0..l.provider_count() {
            add(dkrt_abi::MAX_MODULES + j);
        }
        self.n = n;
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> Manifest for RtManifest<P, M, R> {
    fn tables(&self) -> &[(PackageId, &[u8])] {
        &self.views[..self.n]
    }

    fn core_apis(&self) -> &[Api] {
        self.core.as_slice()
    }

    fn core_resources(&self) -> &[ResourceDecl] {
        &[]
    }

    fn resources(&self, _package: PackageId) -> &[ResourceDecl] {
        &[]
    }

    fn wants(&self, _package: PackageId) -> &[Want] {
        &[]
    }

    fn entries(&self, package: PackageId) -> &[Entry] {
        let s = unsafe { &*self.shared };
        if s.entries_of == Some(package) {
            s.entries.as_slice()
        } else {
            &[]
        }
    }

    fn admit(&mut self, package: PackageId) -> Result<(), Status> {
        unsafe { (*self.shared).admit(package) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::lifecycle::code_of;
    use crate::package::loader::discover::is_runtime_service;
    use crate::test_lock::LOCK;
    use dkneeds::keys::{name_hash, write_table, Need, Table};
    use dkrt_lifecycle::{LifecycleError, LiveSet, PackageGraph};
    use dkrt_sdk::{E_CONTRACT, E_MISSING};
    use std::vec;
    use std::vec::Vec;

    fn needing(service: &str, min_version: u32, hard: bool) -> Vec<u8> {
        let needs = [Need { service: name_hash(service), min_version, hard }];
        let mut b = vec![0u8; 64];
        let n = write_table(&Table { needs: &needs, ..Table::default() }, &mut b).unwrap();
        b.truncate(n);
        b
    }

    /// The code of the refusal of a module that has the one need, in a graph of the core's APIs and nothing else (`None`: it comes up).
    fn refused_with(core: &CoreApis, service: &str, min_version: u32, hard: bool) -> Option<i32> {
        let table = needing(service, min_version, hard);
        let g = PackageGraph::from_tables(&[(PackageId(0), table.as_slice())], core.as_slice()).unwrap();
        g.bring_up_order(&[PackageId(0)], &LiveSet::new()).err().map(|e| code_of(&LifecycleError::Graph(e)))
    }

    #[test]
    fn a_service_file_or_a_loaded_row_is_never_the_cores_and_the_runtime_floor_follows_the_old_rule() {
        let _g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
        // `machines` is a loaded row in the host tests, as it is a `.dks` on the device
        crate::package::svc_rows::test_machines();
        let core = CoreApis::new();
        let has = |n: &str| core.as_slice().iter().any(|a| a.name() == name_hash(n));
        assert!(has("runtime") && has("ui"));
        // `fx` has handle types in the SDK, but a `.dks` provides it: a need of it needs the provider, whatever the table of the runtime holds
        assert!(!has("fx"));
        assert_eq!(refused_with(&core, "fx", 0, true), Some(E_MISSING));
        // a loaded service row is no API of the core: a need of it is not met without its provider
        let rows = crate::package::handlers::services();
        assert!(rows.rows.iter().any(|r| unsafe { r.name_hash() } == dkneeds::keys::name_hash("machines")));
        assert!(!has("machines"));
        assert_eq!(refused_with(&core, "machines", 0, true), Some(E_MISSING));
        // the one rule of the core's names: the boot's search and the admission ask `is_runtime_service`, the graph asks these APIs
        for name in ["runtime", "ui", "fx", "machines", "nothing-like-it"] {
            assert_eq!(is_runtime_service(name_hash(name), &rows), has(name), "{name}");
        }
        // the floor: a need of 0 or of an older runtime is met, a newer one refuses the module (`E_CONTRACT`) when hard and nothing when soft
        let v = dkrt_sdk::RUNTIME_VERSION;
        assert_eq!(refused_with(&core, "runtime", 0, true), None);
        assert_eq!(refused_with(&core, "runtime", v, true), None);
        assert_eq!(refused_with(&core, "runtime", v + 1, true), Some(E_CONTRACT));
        assert_eq!(refused_with(&core, "runtime", v + 1, false), None);
    }
}
