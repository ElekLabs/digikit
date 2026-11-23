//! The loader one package at a time, for the package manager's seams (`lifecycle/`, docs/plans/runtime-contract-and-lifecycle.md section 7). The package manager runs the steps
//! of a load and of a take-down in its own order and unwinds a failure itself; the steps are functions of the loader: `check_head`, `check_body`, `resolve_imports`,
//! `place_image`, `init_and_commit`, `go_live`, `quiesce_service`, `unload_body`. (`dkrt_load` still loads one file through `load_inner` and `complete`, which share the steps.) This file holds the
//! calls that string them together for one package, nothing else. Each call that runs module or service code or reads the selection index sets `BOOT_IDX` to the record for its duration and puts back what it found (`with_boot_idx`).
//!
//! A package id is the record: below `MAX_MODULES` the module record of the selection index with the same number, from there the provider record `id - MAX_MODULES`.
//!
//! The steps of one package up: `package_place` (the file checks but the imports, the image, the record), `package_link` (the imports, the relocations that name them),
//! `package_start` (`init`, the commit, the entries a service installs), `package_activate` (the exports of a service, live: after the entries are in). Down: `package_deactivate`, `package_stop` (`fini`, the registry, the memory of a started record), then
//! `package_free` (the memory of a record that was placed and not started, or whose start failed).
use super::*;
use dkrt_abi::provider::MAX_ENTRIES;
use dkrt_lifecycle::{Cpu, Entry, SiteRef, SlotRef};

// every hook site and slot word that `service_items` can derive is an entry: the bound is the sum of their own bounds, whatever the file lists
const _: () = assert!(MAX_ENTRIES >= SITES_MAX + dkneeds::SLOTS_MAX as usize);

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

/// See `Loader::failure_detail`.
#[derive(Clone, Copy)]
pub(crate) struct FailureDetail {
    pub missing: [u8; NEEDS_LEN],
    pub decl_diff: (u32, u32),
    pub import_hash: u32,
    pub name: [u8; LOG_NAME],
}

fn record_of(id: PackageId) -> usize {
    id.0 as usize
}

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
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
        boot_idx::with(if rec < MAX_MODULES { rec as u32 } else { PROVIDER_IDX }, || f(self))
    }

    /// Place package `id` from the file at `path`: stage it, run every check of a load but the imports, take the memory, copy the image, apply the relocations that name the
    /// image and make the record. The answer is the base of the image. A refusal leaves nothing taken. The DSP range and the relocated DSP image of a file with a DSP section are reserved
    /// here (`dsp::reserve`); the DSP part itself starts after the package is active (`package_start_dsp`).
    pub(crate) unsafe fn package_place(&mut self, id: PackageId, path: *const u8, f: &mut Flight) -> Result<u32, i32> {
        self.with_boot_idx(id, |l| l.place_inner(id, path, f))
    }

    unsafe fn place_inner(&mut self, id: PackageId, path: *const u8, f: &mut Flight) -> Result<u32, i32> {
        let rec = record_of(id);
        if rec >= records::REC_MAX {
            return Err(E_RANGE);
        }
        if crate::package::invariants::bad() {
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
        let name = f.c.f.add(H_NAME);
        let claimed = self.claim_record(slot, name)?;
        debug_assert!(claimed == rec);
        self.torn[rec] = 0;
        let image::Head { image_size, bss, .. } = f.c.head;
        // the DSP range and image come first: nothing else is allocated yet if they fail
        if f.c.dsp_size != 0 {
            let r = dsp::reserve(rec, &f.c.job);
            if r != OK {
                return Err(r);
            }
        }
        let Some(blk) = self.m.alloc(image_size + bss) else {
            if f.c.dsp_size != 0 {
                dsp::cancel(rec);
            }
            return Err(E_NOMEM);
        };
        f.placed = self.place_image(rec, path, &f.c, blk, image::Pass::Base);
        f.package = Some(id);
        Ok(blk.addr.get())
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
        // the records of the loaded service files wait for `package_attach`
        self.split_apply = true;
        let r = self.init_and_commit(rec, keys, f.placed.init, image_size, steps, f.placed.place);
        self.split_apply = false;
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
    /// `Ok(true)` when the package has a DSP part to start once it is active.
    pub(crate) unsafe fn package_activate(&mut self, id: PackageId, f: &mut Flight) -> Result<bool, i32> {
        self.with_boot_idx(id, |l| l.activate_inner(id, f))
    }

    unsafe fn activate_inner(&mut self, id: PackageId, f: &mut Flight) -> Result<bool, i32> {
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
        crate::package::invariants::check();
        Ok(f.c.dsp_size != 0)
    }

    /// Hand the DSP part of the active package `id` to the DSP loader (it comes over the UI passes: `dsp_step`). The reservation was made by `package_place`.
    pub(crate) unsafe fn package_start_dsp(&mut self, id: PackageId) {
        let rec = record_of(id);
        let idx = if rec < MAX_MODULES { self.sel_of(rec) as u32 } else { PROVIDER_IDX };
        dsp::launch(dsp::Pending { rec: rec as u32, reg: 0, reg_size: 0, init: 0, early: 1, boot_idx: idx, send_base: 0, send_len: 0, send_buf: DevAddr::NULL });
    }

    /// The module `consumer` is attached to the services it needs: the walk of the old registry apply over the rows of the in-crate services and the loaded rows of `services`
    /// (`svc_rows::walk_rows`): every row's `apply` in the order of the table, then every record in file order, each to the row that owns its kind (a record of a kind a row does
    /// not own is `OK` there), so that native pages and effect pages interleave as they do in the file and take their reserved rows in that order. The commit of the module
    /// (`init_and_commit` in `package_start`) only keeps the section (`reg_block`). A consumer with no records, or a package that is no module, has nothing to apply. The first
    /// refusal is the answer, after the walk undid what it wrote (`remove` for the consumer on the same rows); the manager does not detach a consumer that was refused.
    pub(crate) unsafe fn package_attach(&mut self, consumer: PackageId, services: &[PackageId]) -> Result<(), i32> {
        // the rows read the selection index (`CoreApi::boot_index`) to find the module whose records they apply
        self.with_boot_idx(consumer, |l| l.attach_inner(consumer, services))
    }

    unsafe fn attach_inner(&mut self, consumer: PackageId, services: &[PackageId]) -> Result<(), i32> {
        let rec = record_of(consumer);
        if rec >= MAX_MODULES {
            return Ok(());
        }
        let (reg, size) = self.reg_block[rec];
        let base = records::module(rec).base;
        let table = ServiceTable { rows: crate::package::svc_rows::walk_rows(services), ..crate::package::handlers::services() };
        if size == 0 {
            // no records: an old row has nothing to apply, but a provider hears every consumer that comes up (`Provider::attach`)
            let c = dkrt_abi::service::ApplyCtx { owner: rec as u8 + 1, reg: core::ptr::null(), base };
            let r = table.attach_providers(&c);
            if r != OK {
                crate::package::registry::remove_with(table, rec as u8 + 1);
                return Err(r);
            }
            return Ok(());
        }
        // a record whose kind a loaded service owns that the module does not need is refused before anything is applied (the module's needs and its builder imports are the same data: a
        // module whose file says otherwise is not one the packer made)
        let mut unowned = false;
        dkrt_abi::registry::each(reg, |kind, _| unowned |= table.len(kind).is_none());
        if unowned {
            return Err(E_MISSING);
        }
        let r = crate::package::registry::apply_with(table, rec as u8 + 1, reg, size, base);
        if r == OK {
            Ok(())
        } else {
            crate::package::registry::remove_with(table, rec as u8 + 1);
            Err(r)
        }
    }

    /// The module `consumer` goes down: the rows of `services` give back what they hold for it (`remove`, the last row of the walk first). The rows of the in-crate services are
    /// asked too; `package_stop` repeats it for every row.
    pub(crate) unsafe fn package_detach(&mut self, consumer: PackageId, services: &[PackageId]) {
        let rec = record_of(consumer);
        if rec >= MAX_MODULES {
            return;
        }
        self.with_boot_idx(consumer, |_| {
            let table = ServiceTable { rows: crate::package::svc_rows::walk_rows(services), ..crate::package::handlers::services() };
            crate::package::registry::remove_with(table, rec as u8 + 1);
        });
    }

    /// What the last failed step of a package left in the loader for a report: the missing module's name, the item that the declarations and the table do not both hold
    /// (`decl_diff`), the name hash that the port file did not list, and the name of the file's header.
    pub(crate) fn failure_detail(&self) -> FailureDetail {
        FailureDetail { missing: self.missing, decl_diff: self.decl_diff, import_hash: self.import_hash, name: self.last_name }
    }

    /// Forget what the last failed step left (`failure_detail`, the name of the last header read included): a load starts with none, so a refusal that no step of the loader made does
    /// not report an earlier package's.
    pub(crate) fn clear_failure_detail(&mut self) {
        self.missing = [0; NEEDS_LEN];
        self.decl_diff = (0, 0);
        self.import_hash = 0;
        self.last_name = [0; LOG_NAME];
    }

    /// Undo `package_activate`, before the entries are restored: a service has no tick, no resident query entry and no export any more. A module has nothing public but
    /// its tick, which `package_stop` stops. Repeating it is harmless.
    pub(crate) unsafe fn package_deactivate(&mut self, id: PackageId) {
        let rec = record_of(id);
        if rec >= MAX_MODULES && records::module(rec).used != 0 {
            self.with_boot_idx(id, |l| {
                l.quiesce_service(rec);
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
        // the DSP slots: the index in the slot block and the pc of the dispatch code (the backend claims them; the DSP loader writes them once the code is acknowledged)
        for &(idx, pc) in &it.dsp_slots[..it.nd] {
            if let Err(e) = out.push(Entry::Slot { slot: SlotRef::new(Cpu::Sharc, idx), target: pc }) {
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

    /// Claim the DSP slots of `entries` for package `id` (`DspInstaller`): the stubs stay stock until the DSP acknowledges the code (`dsp_step`).
    pub(crate) unsafe fn package_install_dsp(&mut self, id: PackageId, entries: &[Entry]) -> Result<(), i32> {
        let mut slots = [(0u32, 0u32); dkneeds::SLOTS_MAX as usize];
        let mut n = 0;
        for e in entries.iter().filter(|e| e.cpu() == Cpu::Sharc) {
            if let Entry::Slot { slot, target } = *e {
                if n == slots.len() {
                    return Err(E_PLAN_FIT);
                }
                slots[n] = (slot.slot(), target);
                n += 1;
            } else {
                return Err(E_FORMAT);
            }
        }
        Backend::install(&mut self.dsp_install, &self.p, record_of(id) as u8, &install::DspItems { slots: &slots[..n] })
    }

    /// Return the DSP slots of package `id` to zero (the clear sequence goes to the DSP): `false` when the DSP does not take it (the caller keeps everything, busy).
    pub(crate) unsafe fn package_restore_dsp(&mut self, id: PackageId) -> bool {
        self.uninstall_dsp(record_of(id))
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
        crate::package::invariants::check();
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
        // a reservation that was never started (the place or link failed after it)
        dsp::cancel(rec);
        self.m.free(DevAddr::new(m.base));
        m.used = 0;
        crate::package::entry::unbind(rec);
        true
    }

    /// The store path of provider `j` of the search, for the log.
    pub(crate) fn provider_file(&self, j: usize) -> &[u8] {
        self.found.path(j)
    }

    /// The selection index of module record `rec` (`u8::MAX`: none).
    pub(crate) fn sel_index(&self, rec: usize) -> u8 {
        if rec < MAX_MODULES {
            self.sel_of(rec)
        } else {
            u8::MAX
        }
    }

    /// The name part of the module id that `capture` read for selection index `k` (zero: no header was read), as the log shows it.
    pub(crate) fn captured_name(&self, k: usize) -> [u8; LOG_NAME] {
        if k < SEL_MAX {
            self.log_name[k]
        } else {
            [0; LOG_NAME]
        }
    }

    /// The boot tried provider `j` and refused it: the live plan counts it as missing from now on.
    pub(crate) fn mark_provider_unusable(&mut self, j: usize) {
        self.unusable[j] = true;
    }

    /// The steps of a take-down that can answer busy, run while the package is whole: no listing or I/O job of the record is running (`unload_body` runs them first, and they are
    /// repeatable), and the DSP rows are cleared (`dsp::quiesce`, which `unload_body` repeats). The live job asks before it takes a module down, so that a busy module still gets its tick back.
    pub(crate) unsafe fn package_retire_ready(&mut self, id: PackageId) -> bool {
        let rec = record_of(id);
        crate::package::files::retire(rec) && crate::package::resident::worker::retire(rec) && retire_sdk(rec) && dsp::quiesce(rec)
    }

    /// Set the loader's phase for the duration of a bring-up that the package manager drives: the services read it (`resident_query` is for after the boot).
    pub(crate) fn set_phase(&mut self, phase: PhaseKind) {
        self.phase = match phase {
            PhaseKind::Closed => Phase::Closed,
            PhaseKind::Boot => Phase::Boot,
            PhaseKind::Live => Phase::Live,
        };
    }
}

/// The phases of `Loader::set_phase`.
#[derive(Clone, Copy)]
pub(crate) enum PhaseKind {
    Closed,
    Boot,
    Live,
}

fn role_hook() -> install::HookItem {
    install::HookItem { addr: 0, len: 0, check: [0; 8], entry: 0 }
}
