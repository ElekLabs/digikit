//! The load of one file into a record: claim, place, `init` and the end of a load (`Loader::load`).
use super::*;

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// Validate and load the `.dkm` at `path`. The tables are checked after every attempt, and none starts once a check has failed (`invariants::check`).
    pub unsafe fn load(&mut self, path: *const u8) -> i32 {
        self.load_file(path, Slot::Free)
    }

    pub(super) unsafe fn load_file(&mut self, path: *const u8, slot: Slot) -> i32 {
        if crate::package::invariants::bad() {
            return E_TABLE;
        }
        let r = self.load_inner(path, slot);
        #[cfg(not(test))]
        crate::package::invariants::check();
        r
    }

    /// `slot`: where the file goes (`Slot`). A service file loads only into a provider record, and a module file never does.
    unsafe fn load_inner(&mut self, path: *const u8, slot: Slot) -> i32 {
        dsp::init_once();
        // the boot loads the selection; after it only a live job loads: into the record of the module a reload replaces, or into a free record
        if !matches!((self.phase, slot), (Phase::Boot, _) | (Phase::Live, Slot::Module(_) | Slot::Free)) {
            return E_BOOT_ONLY;
        }
        let mut c = Checked::empty();
        let r = self.check_file(path, slot, &mut c);
        if r != OK {
            return r;
        }
        let (f, role) = (c.f, c.role);
        let image::Head { image_size, bss, .. } = c.head;
        let steps = RoleSteps::<P, M, R>::of(role);
        let name = f.add(H_NAME);
        let total = image_size + bss;
        let dsp_size = c.dsp_size;
        let job = &c.job;
        // already loaded; a free record
        let rec = match self.claim_record(slot, name) {
            Ok(rec) => rec,
            Err(e) => return e,
        };

        self.torn[rec] = 0;

        // ---- allocate, copy, relocate, zero bss ----
        // The DSP range, the relocated DSP image and the voice rows come first: nothing else is allocated yet if they fail.
        if dsp_size != 0 {
            let r = dsp::reserve(rec, job);
            if r != OK {
                return r;
            }
        }
        let blk = match self.m.alloc(total) {
            Some(b) => b,
            None => {
                if dsp_size != 0 {
                    dsp::cancel(rec);
                }
                return E_NOMEM;
            }
        };
        let base = blk.addr.get();
        let Placed { place, init: initaddr, tick: tickaddr } = self.place_image(rec, path, &c, blk, image::Pass::Both);
        let image::Head { image_size, .. } = c.head;
        let keys = &c.keys;
        let m: &mut Module = records::module(rec);
        if dsp_size != 0 {
            // the CF part goes live now (rows in place before the machine view is built); the DSP code and the voice rows follow
            // when the frames flow
            m.used = 1;
            let r = self.complete(rec, keys, initaddr, tickaddr, image_size, steps, place);
            if r != OK {
                if dsp::retained() { return E_BUSY; }
                dsp::cancel(rec);
                if self.cannot_retire(rec) { return E_BUSY; }
                self.m.free(blk.addr);
                m.used = 0;
                crate::package::entry::unbind(rec);
                return r;
            }
            dsp::launch(dsp::Pending {
                rec: rec as u32,
                reg: 0,
                reg_size: 0,
                init: 0,
                early: 1,
                boot_idx: boot_idx::get(),
                send_base: 0,
                send_len: 0,
                send_buf: DevAddr::NULL,
            });
            return LOADING;
        }
        let r = self.complete(rec, keys, initaddr, tickaddr, image_size, steps, place);
        if r != OK {
            if self.cannot_retire(rec) { return E_BUSY; }
            self.m.free(blk.addr);
            m.used = 0;
            crate::package::entry::unbind(rec);
        }
        r
    }

    /// The record a file named `name` loads into: the slot's own, which must be free (a reload puts the same name back; another name is another module, which the name
    /// rule checks against the other records), or the first free module record. `E_LOADED`, or the refusal of `capacity::pick_record`.
    pub(super) unsafe fn claim_record(&self, slot: Slot, name: *const u8) -> Result<usize, i32> {
        let s = st();
        match slot.fixed() {
            Some(rec) => {
                if records::module(rec).used != 0 {
                    return Err(E_LOADED);
                }
                if let Slot::Module(_) = slot {
                    let mut k = 0;
                    while k < MAX_MODULES {
                        if k != rec && s.modules[k].used != 0 && cstr_eq(s.modules[k].name.as_ptr(), name) {
                            return Err(E_LOADED);
                        }
                        k += 1;
                    }
                }
                Ok(rec)
            }
            None => capacity::pick_record(MAX_MODULES, |i| s.modules[i].used != 0, |i| cstr_eq(s.modules[i].name.as_ptr(), name)),
        }
    }

    /// The image of the checked file `c` into block `blk` (`total` bytes) of record `rec`: copy, zero the bss, the relocations of `pass` (`image::relocate`; the import
    /// addresses of `c` are those of `Both` and `Imports`), the caches, then the record: its name, path, version, base, size, `fini`, entry binding and menu. The record is
    /// in use afterwards.
    pub(super) unsafe fn place_image(&mut self, rec: usize, path: *const u8, c: &Checked, blk: Block, pass: image::Pass) -> Placed {
        let (f, role) = (c.f, c.role);
        let image::Head { needs, image_size, bss, init, fini, menu, tick, .. } = c.head;
        let name = f.add(H_NAME);
        let total = image_size + bss;
        let (mem, base) = (blk.ptr, blk.addr.get());
        image::copy(mem, f, &c.head);
        image::relocate(mem, f, &c.head, base, &c.import_addrs, pass);
        // ---- caches: write the lines back and drop them from both caches ----
        self.p.flush(base, total);

        // ---- record, init, publish ----
        self.roles[rec] = if role == 2 { 2 } else { (role == 1) as u8 };
        if rec < MAX_MODULES {
            let k = boot_idx::get();
            self.sel_of_rec[rec] = if (k as usize) < SEL_MAX { k as u8 + 1 } else { 0 };
        }
        let m: &mut Module = records::module(rec);
        m.used = 1;
        let mut i = 0;
        while i < dkneeds::NAME_LEN as usize {
            m.name[i] = *name.add(i);
            i += 1;
        }
        let dst = records::path(rec);
        let plen = cstr_len(path, 63);
        i = 0;
        while i < 64 {
            dst[i] = if i < plen { *path.add(i) } else { 0 };
            i += 1;
        }
        m.version = rd32(f.add(H_VERSION));
        records::set_runtime(rec, needs & !dkrt_abi::runtime_state::FLAG_TRACE);
        m.base = base;
        m.size = total;
        m.fini = if fini == NONE { 0 } else { base + fini };
        crate::package::entry::bind(rec);
        records::set_menu(rec, if menu == NONE { 0 } else { base + menu });
        let place = PlaceArgs { file: f, keys_off: c.reg_off, keys_len: c.reg_size, image_size, dsp_image: if c.dsp_size != 0 { c.job.image_size } else { 0 }, mem, base, bss };
        Placed { place, init: if init == NONE { 0 } else { base + init }, tick: if tick == NONE { 0 } else { base + tick } }
    }

    /// The end of a load, shared by the immediate and the DSP path (both run in the boot hook; docs/plans/sdk.md 6.3): `init` runs first and its builder calls are collected; then the commit checks them, fills the
    /// voice rows' ranges, writes the registry rows and keeps the record block. Anything that fails undoes what `init` did and writes no row.
    #[allow(clippy::too_many_arguments)]
    unsafe fn complete(&mut self, rec: usize, keys: &crate::package::collect::KeysCopy, init: u32, tick: u32, image_size: u32, steps: RoleSteps<P, M, R>, place: PlaceArgs) -> i32 {
        let r = self.init_and_commit(rec, keys, init, image_size, steps, place);
        if r != OK {
            return r;
        }
        let r = (steps.activate)(self, rec);
        if r != OK {
            return self.fail(rec, r);
        }
        self.go_live(rec, tick);
        OK
    }

    /// The first part of `complete`: the role's place step, `init` with the builder calls collected, then the commit. A failure undoes what was done (`fail`) and is the result.
    #[allow(clippy::too_many_arguments)]
    pub(super) unsafe fn init_and_commit(&mut self, rec: usize, keys: &crate::package::collect::KeysCopy, init: u32, image_size: u32, steps: RoleSteps<P, M, R>, place: PlaceArgs) -> i32 {
        // the role's part of the file that `init` could overwrite in the staging buffer
        let r = (steps.place)(self, rec, &place);
        if r != OK {
            return self.fail(rec, r);
        }
        // the listing client and the I/O jobs are for every record: a service file uses the SDK file calls in its `init` as a module does
        crate::package::files::init_begin(rec);
        let blk = match self.m.alloc(core::mem::size_of::<crate::package::collect::Collect>() as u32) {
            Some(b) => b,
            None => return self.fail(rec, E_NOMEM),
        };
        let block = blk.ptr;
        crate::package::collect::begin(block as *mut crate::package::collect::Collect);
        self.r.module_begin(rec);
        if init != 0 {
            let ctx = crate::package::entry::ctx_ptr(rec);
            CALLED_NEED = [0; NEEDS_LEN];
            let r = self.m.enter_init(rec, init, ctx);
            if r != 0 {
                crate::package::collect::end();
                self.m.free(blk.addr);
                let missing = r == E_MISSING && CALLED_NEED[0] != 0;
                if missing && self.missing[0] == 0 {
                    self.missing = CALLED_NEED;
                }
                let e = self.fail(rec, if missing { E_IMPORT } else { E_INIT });
                return if e == E_BUSY { e } else if missing { E_IMPORT } else { E_INIT };
            }
        }
        crate::package::collect::end();
        let r = self.commit(rec, &*(block as *const crate::package::collect::Collect), keys, image_size);
        self.m.free(blk.addr);
        if r != OK {
            return self.fail(rec, r);
        }
        OK
    }

    /// The last part of `complete`: the record is live. A service file has no intro and is no module of the count; its tick runs as a module's does.
    pub(super) unsafe fn go_live(&mut self, rec: usize, tick: u32) {
        let s = st();
        let module = rec < MAX_MODULES;
        let m: &mut Module = records::module(rec);
        m.used = 1;
        if tick != 0 {
            crate::package::entry::publish_tick(rec, tick);
        }
        crate::package::files::ready(rec);
        if module {
            s.loads = s.loads.wrapping_add(1);
            crate::package::resident::intro::commit(rec);
            crate::package::module_ui::publish(rec);
        }
    }

    /// Undo what `init` and the checks before the commit did for record `rec`; `r` is the result to return (`E_BUSY` when a step of the module is still running).
    pub(super) unsafe fn fail(&mut self, rec: usize, r: i32) -> i32 {
        let module = rec < MAX_MODULES;
        if module {
            crate::package::resident::intro::discard(rec);
            crate::package::module_ui::withdraw(rec);
        }
        let held = !crate::package::files::retire(rec) | !retire_sdk(rec);
        crate::package::module_cmds::release_cmds(rec as u8 + 1);
        crate::package::resident::dlog::release_sinks(rec);
        (RoleSteps::<P, M, R>::of(self.roles[rec] as u32).undo)(self, rec);
        self.r.remove(rec);
        if module && !dsp::release(rec) || held {
            return E_BUSY;
        }
        r
    }

    /// A file or I/O step of the record still runs, or the DSP loader keeps a module's rows: the record must not be freed yet.
    pub(super) unsafe fn cannot_retire(&mut self, rec: usize) -> bool {
        (rec < MAX_MODULES && dsp::retained()) || !crate::package::files::retire(rec) || !retire_sdk(rec)
    }
}
