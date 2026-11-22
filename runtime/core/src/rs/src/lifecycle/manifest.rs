use super::session::Session;
use crate::loader::{Images, Registry};
use dkrt_abi::package::PackageId;
use dkrt_abi::platform::Platform;
use dkrt_abi::REC_MAX;
use dkrt_fit::ledger::{ResourceDecl, Want};
use dkrt_lifecycle::{Api, Entry, Manifest};

/// The header tables of the selected packages and of the services they need, as the planner keeps them: the module at each selection index (`Loader::capture`), the
/// provider the search found (`discover::find`). The tables stay where the loader holds them; the manifest holds views of them, so the loader's tables must not change while
/// the manager lives. The core's APIs are the runtime floor and the services of the image (`HANDLE_SERVICES`, the in-crate rows), all at the runtime's version.
///
/// Today's headers pack no pools and no claims, so the manifest has none (RC3 packs them). The entries of a package are those its start derived (`Session`), and are valid
/// from the start of that package to the start of the next, which is when the manager installs them.
pub struct RtManifest<P: Platform, M: Images, R: Registry> {
    shared: *const Session<P, M, R>,
    views: [(PackageId, &'static [u8]); REC_MAX],
    n: usize,
    core: [Api; CORE_MAX],
    ncore: usize,
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

/// The APIs of the core: the names of `HANDLE_SERVICES` and of the in-crate rows.
const CORE_MAX: usize = 16;

impl<P: Platform, M: Images, R: Registry> RtManifest<P, M, R> {
    /// The manifest of the first `modules` selection indices and the providers of the search, over the loader of `shared`.
    ///
    /// # Safety
    /// `shared` and its loader outlive the manifest, and the loader's kept tables do not change while it lives.
    pub unsafe fn of(shared: *const Session<P, M, R>, modules: usize) -> Self {
        let mut core = [Api::new(0, 0); CORE_MAX];
        let mut ncore = 0;
        let version = dkrt_sdk::RUNTIME_VERSION;
        for name in dkrt_sdk::records::HANDLE_SERVICES {
            core[ncore] = Api::new(dkneeds::keys::name_hash(name), version);
            ncore += 1;
        }
        for row in &crate::svc_rows::rows()[..crate::gen_services::STATIC] {
            core[ncore] = Api::new(dkneeds::keys::name_hash((**row).name()), version);
            ncore += 1;
        }
        let mut m = RtManifest { shared, views: [(PackageId(0), &[]); REC_MAX], n: 0, core, ncore };
        m.reread(modules);
        m
    }

    /// Read the loader's kept tables again (a file was added or replaced since): the first `modules` selection indices and the providers of the search. A table that the shared
    /// parser does not accept is left out, so that one bad file does not refuse the whole graph; its package then has no node and is refused alone.
    ///
    /// # Safety
    /// As for [`RtManifest::of`].
    pub unsafe fn reread(&mut self, modules: usize) {
        let l = &*(*self.shared).loader;
        let empty = empty_table();
        let mut n = 0;
        let mut add = |id: usize, module: bool| {
            let table = match l.package_table(PackageId(id as u8)) {
                Some(t) if dkneeds::keys::Keys::parse(t).is_ok() => Some(t),
                _ if module => Some(empty),
                _ => None,
            };
            if let Some(t) = table {
                // a view of the loader's own array (or of the empty table), which outlives the manifest
                self.views[n] = (PackageId(id as u8), core::slice::from_raw_parts(t.as_ptr(), t.len()));
                n += 1;
            }
        };
        for k in 0..modules.min(dkrt_abi::MAX_MODULES) {
            add(k, true);
        }
        for j in 0..l.provider_count() {
            add(dkrt_abi::MAX_MODULES + j, false);
        }
        self.n = n;
    }
}

impl<P: Platform, M: Images, R: Registry> Manifest for RtManifest<P, M, R> {
    fn tables(&self) -> &[(PackageId, &[u8])] {
        &self.views[..self.n]
    }

    fn core_apis(&self) -> &[Api] {
        &self.core[..self.ncore]
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
}
