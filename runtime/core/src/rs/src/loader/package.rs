//! The loader one package at a time, for the package manager's seams (`lifecycle/`, docs/plans/runtime-contract-and-lifecycle.md section 7). The boot's loops run the
//! steps of `load_inner`, `complete` and `unload_rec_inner` for a record one after the other; the package manager runs the same steps with its own order and unwind. The steps are
//! the same functions: `check_head`, `check_body`, `resolve_imports`, `place_image`, `init_and_commit`, `go_live`, `quiesce_service`, `unload_body`. This file holds the
//! calls that string them together for one package, nothing else. Each call that runs module or service code or reads the selection index sets `BOOT_IDX` to the record for its duration and puts back what it found (`with_boot_idx`). Nothing in the runtime calls them yet (RC2b-2).
//!
//! A package id is the record: below `MAX_MODULES` the module record of the selection index with the same number, from there the provider record `id - MAX_MODULES`.
//!
//! The steps of one package up: `package_place` (the file checks but the imports, the image, the record), `package_link` (the imports, the relocations that name them),
//! `package_start` (`init`, the commit, the entries a service installs), `package_activate` (the exports of a service, live: after the entries are in). Down: `package_deactivate`, `package_stop` (`fini`, the registry, the memory of a started record), then
//! `package_free` (the memory of a record that was placed and not started, or whose start failed).
use super::*;
use dkrt_abi::provider::MAX_ENTRIES;
use dkrt_lifecycle::{Cpu, Entry, SiteRef, SlotRef};

/// What a package carries from its place to its start: the checked file (the staged bytes stay in the staging buffer until the next `stage`) and where the image went.
pub(crate) struct Flight {
    c: Checked,
    placed: Placed,
    /// The package `c` is of.
    package: Option<PackageId>,
}

impl Flight {
    pub(crate) const fn new() -> Flight {
        Flight { c: Checked::empty(), placed: Placed { place: PlaceArgs { file: core::ptr::null(), keys_off: 0, keys_len: 0, image_size: 0, dsp_image: 0, mem: core::ptr::null_mut(), base: 0, bss: 0 }, init: 0, tick: 0 }, package: None }
    }
}

/// The entries of one package: what `package_start` derived from its table and the port file (hook sites and ColdFire slot words; the DSP slots are the DSP part's).
pub(crate) struct Entries {
    n: usize,
    v: [Entry; MAX_ENTRIES],
}

impl Entries {
    pub(crate) const fn new() -> Entries {
        Entries { n: 0, v: [Entry::Slot { slot: SlotRef::new(Cpu::ColdFire, 0), target: 0 }; MAX_ENTRIES] }
    }

    pub(crate) fn as_slice(&self) -> &[Entry] {
        &self.v[..self.n]
    }

    fn push(&mut self, e: Entry) -> Result<(), i32> {
        if self.n == MAX_ENTRIES {
            return Err(E_PLAN_FIT);
        }
        self.v[self.n] = e;
        self.n += 1;
        Ok(())
    }
}

fn record_of(id: PackageId) -> usize {
    id.0 as usize
}

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// The header table of package `id` that the planner kept: the module of the selection index (`capture`), the provider the search found. `None` when there is none.
    pub(crate) fn package_table(&self, id: PackageId) -> Option<&[u8]> {
        let rec = record_of(id);
        if rec < SEL_MAX.min(MAX_MODULES) {
            return if self.table_len[rec] != 0 { Some(&self.tables[rec][..self.table_len[rec]]) } else { None };
        }
        let j = rec.checked_sub(MAX_MODULES)?;
        if j < self.found.n.min(PROVIDER_MAX) && self.found.table_len[j] != 0 {
            Some(self.found.table(j))
        } else {
            None
        }
    }

    /// How many providers the search found (at most `PROVIDER_MAX`).
    pub(crate) fn provider_count(&self) -> usize {
        self.found.n.min(PROVIDER_MAX)
    }

    /// The store path of provider `j` of the search, NUL terminated into `out`.
    pub(crate) fn provider_path(&self, id: PackageId, out: &mut [u8; discover::PATH_CAP + 1]) -> bool {
        let Some(j) = record_of(id).checked_sub(MAX_MODULES) else {
            return false;
        };
        if j >= self.found.n.min(PROVIDER_MAX) {
            return false;
        }
        let at = self.found.path(j);
        out[..at.len()].copy_from_slice(at);
        out[at.len()] = 0;
        true
    }

    /// Run `f` with `BOOT_IDX` (the selection index the services read to find the module that loads) set to the record of package `id`, and put back the value found.
    unsafe fn with_boot_idx<T>(&mut self, id: PackageId, f: impl FnOnce(&mut Self) -> T) -> T {
        let rec = record_of(id);
        let was = core::ptr::read_volatile(core::ptr::addr_of!(BOOT_IDX));
        crate::selection::BOOT_IDX = if rec < MAX_MODULES { rec as u32 } else { PROVIDER_IDX };
        let r = f(self);
        crate::selection::BOOT_IDX = was;
        r
    }

    /// Place package `id` from the file at `path`: stage it, run every check of a load but the imports, take the memory, copy the image, apply the relocations that name the
    /// image and make the record. The answer is the base of the image. A refusal leaves nothing taken. A file with a DSP section is refused (`E_CONTEXT`): the DSP part
    /// comes up over the UI passes, which the package manager does not drive yet.
    pub(crate) unsafe fn package_place(&mut self, id: PackageId, path: *const u8, f: &mut Flight) -> Result<u32, i32> {
        self.with_boot_idx(id, |l| l.place_inner(id, path, f))
    }

    unsafe fn place_inner(&mut self, id: PackageId, path: *const u8, f: &mut Flight) -> Result<u32, i32> {
        let rec = record_of(id);
        if rec >= records::REC_MAX {
            return Err(E_RANGE);
        }
        if crate::invariants::bad() {
            return Err(E_TABLE);
        }
        dsp::init_once();
        let slot = if rec < MAX_MODULES { Slot::Module(rec) } else { Slot::Provider(rec) };
        self.missing = [0; NEEDS_LEN];
        self.decl_diff = (0, 0);
        self.import_hash = 0;
        f.package = None;
        f.c = Checked::empty();
        let r = self.check_head(path, slot, &mut f.c);
        if r != OK {
            return Err(r);
        }
        let r = self.check_body(&mut f.c);
        if r != OK {
            return Err(r);
        }
        if f.c.dsp_size != 0 {
            return Err(E_CONTEXT);
        }
        let name = f.c.f.add(H_NAME);
        let claimed = self.claim_record(slot, name)?;
        debug_assert!(claimed == rec);
        self.torn[rec] = 0;
        let image::Head { image_size, bss, .. } = f.c.head;
        let blk = self.m.alloc(image_size + bss).ok_or(E_NOMEM)?;
        f.placed = self.place_image(rec, path, &f.c, blk, image::Pass::Base);
        f.package = Some(id);
        Ok(blk.addr)
    }

    /// Bind the imports of the placed package `id` to `services`, the packages of its needs: the import addresses are those of the runtime's tables, the port file, or an
    /// export of one of `services` (`resolve_imports`), and the relocations that name them are applied. A refusal leaves the placed record as it was.
    pub(crate) unsafe fn package_link(&mut self, id: PackageId, services: &[PackageId], f: &mut Flight) -> Result<(), i32> {
        self.with_boot_idx(id, |l| l.link_inner(id, services, f))
    }

    unsafe fn link_inner(&mut self, id: PackageId, services: &[PackageId], f: &mut Flight) -> Result<(), i32> {
        debug_assert!(f.package == Some(id));
        let r = self.resolve_imports(&mut f.c, Some(services));
        if r != OK {
            return Err(r);
        }
        let image::Head { image_size, bss, .. } = f.c.head;
        let p = &f.placed.place;
        image::relocate(p.mem, f.c.f, &f.c.head, p.base, &f.c.import_addrs, image::Pass::Imports);
        self.p.flush(p.base, image_size + bss);
        Ok(())
    }

    /// Start the placed and linked package `id`: the role's place step, `init`, the commit and, for a service, the entries it installs into `entries`. Nothing is public
    /// afterwards: the exports and the live record are `package_activate`, after the entries are in (the order of the boot's load). A failure undoes what the steps did
    /// (`fail`); the record stays placed for `package_free`.
    pub(crate) unsafe fn package_start(&mut self, id: PackageId, f: &mut Flight, entries: &mut Entries) -> Result<(), i32> {
        self.with_boot_idx(id, |l| l.start_inner(id, f, entries))
    }

    unsafe fn start_inner(&mut self, id: PackageId, f: &mut Flight, entries: &mut Entries) -> Result<(), i32> {
        debug_assert!(f.package == Some(id));
        let rec = record_of(id);
        let role = f.c.role;
        let steps = RoleSteps::<P, M, R>::of(role);
        let image::Head { image_size, .. } = f.c.head;
        let keys = &f.c.keys;
        let r = self.init_and_commit(rec, keys, f.placed.init, image_size, steps, f.placed.place);
        if r != OK {
            return Err(r);
        }
        *entries = Entries::new();
        if role == 1 {
            let r = self.derive_entries(rec, entries);
            if r != OK {
                return Err(self.fail(rec, r));
            }
        }
        Ok(())
    }

    /// Make the started package `id` public: a service publishes its exports and its resident query entry, then the record is live (tick, listing client, intro). Called
    /// after the entries are installed. A refusal leaves nothing published.
    pub(crate) unsafe fn package_activate(&mut self, id: PackageId, f: &mut Flight) -> Result<(), i32> {
        self.with_boot_idx(id, |l| l.activate_inner(id, f))
    }

    unsafe fn activate_inner(&mut self, id: PackageId, f: &mut Flight) -> Result<(), i32> {
        debug_assert!(f.package == Some(id));
        let rec = record_of(id);
        if f.c.role == 1 {
            match role::service_publish(self, rec) {
                Ok(q) => self.provs[rec - MAX_MODULES].query_v1 = q.unwrap_or(0),
                Err(e) => {
                    self.exports.withdraw(rec);
                    return Err(e);
                }
            }
        }
        self.go_live(rec, f.placed.tick);
        #[cfg(not(test))]
        crate::invariants::check();
        Ok(())
    }

    /// Undo `package_activate`, before the entries are restored: a service has no tick, no resident query entry and no export any more. A module has nothing public but
    /// its tick, which `package_stop` stops. Repeating it is harmless.
    pub(crate) unsafe fn package_deactivate(&mut self, id: PackageId) {
        let rec = record_of(id);
        if rec >= MAX_MODULES && records::module(rec).used != 0 {
            self.with_boot_idx(id, |l| {
                l.quiesce_service(rec, false);
                l.exports.withdraw(rec);
            });
        }
    }

    /// The entries of the committed service `rec` (`ServiceItems` as hook and slot entries at the absolute addresses of the placed image). A hook entry past the image is
    /// `E_RANGE`, as the installer of the boot's load refuses it.
    unsafe fn derive_entries(&mut self, rec: usize, out: &mut Entries) -> i32 {
        let mut it = role::ServiceItems::empty();
        match role::service_items(self, rec, &mut it) {
            Ok(true) => {}
            Ok(false) => return OK,
            Err(e) => return e,
        }
        let base = records::module(rec).base;
        let image_size = self.provs[rec - MAX_MODULES].image_size;
        for h in &it.hooks[..it.nh] {
            if h.entry >= image_size {
                return E_RANGE;
            }
            if let Err(e) = out.push(Entry::Hook { site: SiteRef::new(Cpu::ColdFire, h.addr), target: base.wrapping_add(h.entry) }) {
                return e;
            }
        }
        for &(word, value) in &it.slots[..it.ns] {
            if let Err(e) = out.push(Entry::Slot { slot: SlotRef::new(Cpu::ColdFire, word), target: value }) {
                return e;
            }
        }
        OK
    }

    /// The length and check value of the hook site at `addr` in the loaded port file: what the ColdFire installer needs besides the address and the target of a hook entry.
    pub(crate) unsafe fn port_site_at(&self, addr: u32) -> Option<(u32, [u8; 8])> {
        let (keys, ..) = self.port_keys()?;
        let mut i = 0;
        while let Some(s) = keys.site(i) {
            i += 1;
            if s.addr == addr {
                return Some((s.len as u32, s.check));
            }
        }
        None
    }

    /// Install the ColdFire entries of `entries` for package `id`: the hook sites and slot words, all or nothing (`Installer::install`). The targets are absolute addresses.
    pub(crate) unsafe fn package_install(&mut self, id: PackageId, entries: &[Entry]) -> Result<(), i32> {
        let rec = record_of(id);
        // the image of the package: a hook target must lie in it (a module has no hook: its image size is 0)
        let (base, image_size) = if rec >= MAX_MODULES { (records::module(rec).base, self.provs[rec - MAX_MODULES].image_size) } else { (0, 0) };
        let mut hooks = [role_hook(); SITES_MAX];
        let mut slots = [(0u32, 0u32); dkneeds::SLOTS_MAX as usize];
        let (mut nh, mut ns) = (0, 0);
        // the entries of this CPU; the others are another backend's
        for e in entries.iter().filter(|e| e.cpu() == Cpu::ColdFire) {
            match *e {
                Entry::Hook { site, target } => {
                    let (len, check) = self.port_site_at(site.addr()).ok_or(E_IMPORT)?;
                    if nh == hooks.len() {
                        return Err(E_PLAN_FIT);
                    }
                    hooks[nh] = install::HookItem { addr: site.addr(), len, check, entry: target.wrapping_sub(base) };
                    nh += 1;
                }
                Entry::Slot { slot, target } => {
                    if ns == slots.len() {
                        return Err(E_PLAN_FIT);
                    }
                    slots[ns] = (slot.slot(), target);
                    ns += 1;
                }
                // a table pointer or bound written as bytes: the installers of RC4 do it
                Entry::Patch { .. } => return Err(E_FORMAT),
            }
        }
        // the entries carry absolute targets; the installer takes the offset in the image (a target before the image wraps past its size) and refuses one outside it
        let cf = CfItems { hooks: &hooks[..nh], image_base: base, image_size, slots: &slots[..ns] };
        Backend::install(&mut self.installer, &self.p, rec as u8, &cf)
    }

    /// Return the hook sites and slot words of package `id` to stock.
    pub(crate) unsafe fn package_restore(&mut self, id: PackageId) {
        self.uninstall_cf(record_of(id));
    }

    /// Take the started package `id` down (after `package_deactivate` and the restore of its entries): `fini`, the registry, the DSP rows, the memory (`unload_body`). The entries are restored before, by
    /// the package manager. `E_BUSY`: a file or I/O step of the record still runs; the call is repeated (the steps that ran are not run again).
    pub(crate) unsafe fn package_stop(&mut self, id: PackageId) -> i32 {
        self.with_boot_idx(id, |l| l.stop_inner(id))
    }

    unsafe fn stop_inner(&mut self, id: PackageId) -> i32 {
        let rec = record_of(id);
        if records::module(rec).used == 0 {
            return OK;
        }
        let r = self.unload_body(rec);
        #[cfg(not(test))]
        crate::invariants::check();
        r
    }

    /// Release the memory of package `id` when `package_stop` did not (the record was placed and not started, or its start failed): the record goes unused and its entries
    /// are unbound, as a failed load does. `false` when a file or I/O step of the record still runs (`E_BUSY` of a load): nothing is released and the call is repeated.
    pub(crate) unsafe fn package_free(&mut self, id: PackageId) -> bool {
        let rec = record_of(id);
        let m: &mut Module = records::module(rec);
        if m.used == 0 {
            return true;
        }
        if self.cannot_retire(rec) {
            return false;
        }
        self.m.free(m.base);
        m.used = 0;
        crate::entry::unbind(rec);
        true
    }
}

fn role_hook() -> install::HookItem {
    install::HookItem { addr: 0, len: 0, check: [0; 8], entry: 0 }
}
