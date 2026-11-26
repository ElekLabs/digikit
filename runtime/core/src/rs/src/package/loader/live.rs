//! The checks of a live job over the live set of modules, before anything changes (`Loader::reload_check`, `Loader::live_check_load`), and the admission state a live load
//! starts from (`Loader::seed_live`). The needs of the new file are asked of the package graph (`dkrt-lifecycle`), as the manager asks them when it loads the file; the rules of the
//! core (owners, keys, capacity) are `Admission`, over every live module with the new table at its index.
use super::*;
use crate::package::admission::Admission;
use dkrt_lifecycle::{LiveSet, PackageGraph};
use core::mem::MaybeUninit;

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// The selection indices of the modules that are loaded, and `extra` (the index a reload is about to load into).
    pub(super) unsafe fn live_at(&self, extra: usize) -> [bool; SEL_MAX] {
        let mut live = [false; SEL_MAX];
        let mut r = 0;
        while r < MAX_MODULES {
            let at = self.sel_of(r) as usize;
            if records::module(r).used != 0 && at < SEL_MAX {
                live[at] = true;
            }
            r += 1;
        }
        if extra < SEL_MAX {
            live[extra] = true;
        }
        live
    }

    /// The live modules' tables by selection index, and `None` for the others.
    unsafe fn live_modules(&self, except: usize) -> [Option<Keys<'_>>; SEL_MAX] {
        let mut mods: [Option<Keys>; SEL_MAX] = [None; SEL_MAX];
        let mut r = 0;
        while r < MAX_MODULES {
            let at = self.sel_of(r) as usize;
            if records::module(r).used != 0 && at < SEL_MAX && at != except && self.table_len[at] != 0 {
                mods[at] = Keys::parse(&self.tables[at][..self.table_len[at]]).ok();
            }
            r += 1;
        }
        mods
    }

    /// The needs of the table `new`, as the package manager would meet them: the module at selection index `k` and the providers the boot found (but not those it refused), in a
    /// graph of their own. `OK`, or the planner's code for the first need that nothing meets (`E_MISSING`) or that a provider meets at an older major (`E_CONTRACT`).
    unsafe fn live_needs(&self, k: usize, new: &[u8]) -> i32 {
        let mut tables: [(PackageId, &[u8]); PROVIDER_MAX + 1] = [(PackageId(0), &[]); PROVIDER_MAX + 1];
        tables[0] = (PackageId(k as u8), new);
        let mut n = 1;
        let mut j = 0;
        while j < self.found.n.min(PROVIDER_MAX) {
            if !self.unusable[j] && Keys::parse(self.found.table(j)).is_ok() {
                tables[n] = (PackageId((MAX_MODULES + j) as u8), self.found.table(j));
                n += 1;
            }
            j += 1;
        }
        let core = crate::package::lifecycle::CoreApis::new();
        let mut g = MaybeUninit::<PackageGraph>::uninit();
        if let Err(r) = PackageGraph::init_from_tables(g.as_mut_ptr(), &tables[..n], core.as_slice()) {
            return crate::package::lifecycle::code_of(&dkrt_lifecycle::LifecycleError::Graph(r));
        }
        match (*g.as_ptr()).bring_up_order(&[PackageId(k as u8)], &LiveSet::new()) {
            Ok(_) => OK,
            Err(r) => crate::package::lifecycle::code_of(&dkrt_lifecycle::LifecycleError::Graph(r)),
        }
    }

    /// Search the card for the services the table `new` needs that no provider of the boot's search offers (the boot looks only for what its selection needs, so a service file
    /// transferred since, or never needed by the selection, is not in `found`). The providers found are added after the others (`discover::find_more`), so the graph the live
    /// check and the manager build holds them; the manager brings them up when it loads the module, through the checks of every service. A need the card does not meet stays
    /// unmet (`E_MISSING`).
    unsafe fn live_find(&mut self, new: &[u8]) -> i32 {
        let Ok(t) = Keys::parse(new) else { return OK };
        let mut want = [0u32; plan::WANT_MAX];
        let nw = plan::unmet_needs(&[Some(t)], &mut want);
        let mut missing = [0u32; plan::WANT_MAX];
        let mut nm = 0;
        let mut i = 0;
        while i < nw {
            let mut met = false;
            let mut j = 0;
            while j < self.found.n.min(PROVIDER_MAX) {
                if let Ok(p) = Keys::parse(self.found.table(j)) {
                    let mut w = 0;
                    while let Some(h) = p.words(dkneeds::keys::tag::PROVIDES, w) {
                        met |= h.0 == want[i];
                        w += 1;
                    }
                }
                j += 1;
            }
            if !met {
                missing[nm] = want[i];
                nm += 1;
            }
            i += 1;
        }
        if nm != 0 {
            return discover::find_more(&mut self.m, &missing[..nm], dkrt_abi::fw_id::image_fw_id(&self.p), &mut self.found);
        }
        OK
    }

    /// Would the load of the table `new` bring up, at run time, a service that keeps per-project state? Such a service registers the settings overlay (its file imports
    /// `dkc_settings_overlay_set`) and learns the current project only from the next project load event, so a bring-up in the middle of a project would run it unbound.
    /// The firmware's current project is not cheap to read from here, so the load is refused (`E_BOOT_ONLY`: the service belongs to the boot's set) and a restart brings it up
    /// bound. Only the providers the new file reaches through its hard needs count: found, not up, usable.
    unsafe fn live_brings_up_overlay(&mut self, new: &[u8]) -> bool {
        use dkneeds::keys::tag;
        let Ok(t) = Keys::parse(new) else { return false };
        let mut reach = [0u32; plan::WANT_MAX];
        let mut nr = plan::unmet_needs(&[Some(t)], &mut reach);
        let mut seen = [false; PROVIDER_MAX];
        let mut grew = true;
        while grew {
            grew = false;
            let mut j = 0;
            while j < self.found.n.min(PROVIDER_MAX) {
                if !seen[j] && !self.unusable[j] && records::module(MAX_MODULES + j).used == 0 {
                    let mut hit = false;
                    if let Ok(p) = Keys::parse(self.found.table(j)) {
                        let mut w = 0;
                        while let Some(h) = p.words(tag::PROVIDES, w) {
                            hit |= reach[..nr].contains(&h.0);
                            w += 1;
                        }
                        if hit {
                            let mut i = 0;
                            while let Some(n) = p.words(tag::NEEDS, i) {
                                i += 1;
                                if n.2 & 1 != 0 && !reach[..nr].contains(&n.0) && nr < plan::WANT_MAX {
                                    reach[nr] = n.0;
                                    nr += 1;
                                }
                            }
                        }
                    }
                    if hit {
                        seen[j] = true;
                        grew = true;
                        let at = self.found.path(j).len();
                        let mut path = [0u8; discover::PATH_CAP + 1];
                        path[..at].copy_from_slice(self.found.path(j));
                        if let Ok(s) = self.m.stage(path.as_ptr()) {
                            if imports_name(core::slice::from_raw_parts(s.ptr, s.size as usize), b"dkc_settings_overlay_set") {
                                return true;
                            }
                        }
                    }
                }
                j += 1;
            }
        }
        false
    }

    /// Is the import `import` (name hash), which the last file check could not resolve (`import_name`), exported by a provider of `found` that is not up yet? Only then does the
    /// refusal wait for the manager, which brings the service up and links the module; any other unresolved import stays a refusal.
    unsafe fn needs_service_down(&self, import: u32) -> bool {
        if import == 0 {
            return false;
        }
        let mut j = 0;
        while j < self.found.n.min(PROVIDER_MAX) {
            if let Ok(p) = Keys::parse(self.found.table(j)) {
                let mut w = 0;
                while let Some(h) = p.words(dkneeds::keys::tag::EXPORTS, w) {
                    if h.0 == import && !self.unusable[j] && records::module(MAX_MODULES + j).used == 0 {
                        return true;
                    }
                    w += 1;
                }
            }
            j += 1;
        }
        false
    }

    /// The live check of the table `new` at selection index `k` (the old module's slot for a reload, the index of a load): `OK`, or the refusal of its needs, then of the file by
    /// the rules of the core, then of any live module that no longer fits with it (`caps`: `live_caps`, which counts every live module again).
    pub(super) unsafe fn live_plan(&self, k: usize, new: &[u8], caps: &[DkrtCap]) -> i32 {
        let r = self.live_needs(k, new);
        if r != OK {
            return r;
        }
        let Ok(new) = Keys::parse(new) else { return E_FORMAT };
        let mut mods = self.live_modules(k);
        mods[k] = Some(new);
        let np = self.found.n.min(PROVIDER_MAX);
        let mut provs = parse_provs(&self.found, np);
        let mut adm = Admission::new(caps);
        let mut j = 0;
        while j < np {
            if self.unusable[j] {
                provs[j] = None;
            }
            adm.provider_is_there(j);
            j += 1;
        }
        let mut verdicts = [Verdict::Accepted; SEL_MAX];
        let mut i = 0;
        while i < SEL_MAX {
            if mods[i].is_some() {
                verdicts[i] = adm.judge_module(&services(), &mods, &provs[..np], i);
            }
            i += 1;
        }
        // the new file first, then any live module that no longer fits with it
        if !verdicts[k].is_accepted() {
            return verdicts[k].result();
        }
        let mut i = 0;
        while i < SEL_MAX {
            if mods[i].is_some() && !verdicts[i].is_accepted() {
                return verdicts[i].result();
            }
            i += 1;
        }
        OK
    }

    /// The admission a live load of the module at selection index `k` starts from: the providers that are up are there (they were judged when they came up and their uses are in
    /// `caps`, `live_caps` with nothing given back), and every other live module has taken its share, in index order.
    pub(crate) unsafe fn seed_live(&self, adm: &mut Admission, k: usize) {
        let np = self.found.n.min(PROVIDER_MAX);
        let provs = parse_provs(&self.found, np);
        let mut j = 0;
        while j < np {
            if records::module(MAX_MODULES + j).used != 0 {
                adm.provider_is_there(j);
            }
            j += 1;
        }
        let mods = self.live_modules(k);
        let mut i = 0;
        while i < SEL_MAX {
            if mods[i].is_some() {
                adm.judge_module(&services(), &mods, &provs[..np], i);
            }
            i += 1;
        }
    }
}

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// Does the module or service file `file` list `name` among its imports?
fn imports_name(file: &[u8], name: &[u8]) -> bool {
    let be = |p: usize| file.get(p..p + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let (Some(off), Some(n)) = (be(dkneeds::H_IMPORT_OFF as usize), be(dkneeds::H_IMPORT_N as usize)) else { return false };
    let len = dkneeds::CF_IMPORT_LEN as usize;
    let mut i = 0;
    while i < n {
        let Some(e) = file.get(off + i * len..off + (i + 1) * len) else { return false };
        let end = e.iter().position(|b| *b == 0).unwrap_or(len);
        if &e[..end] == name {
            return true;
        }
        i += 1;
    }
    false
}

/// The checks of a reload of module record `rec` by the file at `path`, before anything is unloaded: nothing changes but the staging buffer. In this order: the
    /// record is a live module of the boot selection; the file passes every check of a load (`check_file`), has the same name, a header table the planner can keep, and
    /// every key of the old file (`E_RELOAD_KEYS`); the planner accepts the live set with the file in the old module's slot (`caps`: `live_caps`). `OK`, or the refusal.
    ///
    /// The checks of a file read the numbers the services gave its keys (`resolve` of the machines service names a voice's type by the selection index of the module that loads,
    /// `CoreApi::boot_index`). A load has that index set and the keys numbered by `settle`; so the check sets the index to the module's, and numbers the file's keys first (the old
    /// keys keep their numbers, a key the file adds takes a free one, as `reload_load` does again). The numbering is done only for a file that has the module's name and every old key.
    pub unsafe fn reload_check(&mut self, rec: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        if rec >= MAX_MODULES || records::module(rec).used == 0 {
            return E_NOT_FOUND;
        }
        let k = self.sel_of(rec) as usize;
        if k >= SEL_MAX || self.table_len[k] == 0 {
            return E_NOT_FOUND;
        }
        if let Ok(staged) = self.m.stage(path) {
            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
            if let Some(table) = table_of_file(file) {
                let same_name = file.len() >= H_NAME + dkneeds::NAME_LEN as usize && file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] == records::module(rec).name[..];
                if let (true, Ok(old), Ok(new)) = (same_name, Keys::parse(&self.tables[k][..self.table_len[k]]), Keys::parse(table)) {
                    if old.first_missing_key(&new).is_none() {
                        self.number_keys(k, file);
                    }
                }
            }
        }
        boot_idx::with(k as u32, || self.reload_check_inner(rec, k, path, caps))
    }

    unsafe fn reload_check_inner(&mut self, rec: usize, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        let mut c = Checked::empty();
        let r = self.check_file(path, Slot::Module(rec), &mut c);
        if r != OK {
            return r;
        }
        if !cstr_eq(c.f.add(H_NAME), records::module(rec).name.as_ptr()) {
            return E_FORMAT; // another name is another module
        }
        if c.reg_size as usize > TABLE_MAX {
            return E_TOO_BIG;
        }
        let new = match Keys::parse(core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize)) {
            Ok(t) => t,
            Err(_) => return E_FORMAT,
        };
        match Keys::parse(&self.tables[k][..self.table_len[k]]) {
            Ok(old) => {
                if old.first_missing_key(&new).is_some() {
                    return E_RELOAD_KEYS;
                }
            }
            Err(_) => return E_NOT_FOUND,
        }
        self.live_plan(k, core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize), caps)
    }

    /// The part of `reload_load` before the file loads: the rules of the check for the bytes that load, the file's header table kept at the module's index, the
    /// services' numbering. The package manager loads the file after it.
    pub(crate) unsafe fn reload_prepare(&mut self, rec: usize, path: *const u8) -> i32 {
        let k = if rec < MAX_MODULES { self.sel_of(rec) as usize } else { SEL_MAX };
        if k >= SEL_MAX {
            return E_NOT_FOUND;
        }
        // the file can have changed since `reload_check` (a host writes it while the module drains): the rules of the check hold for the bytes that load
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
        if file.len() < dkneeds::HEADER as usize || file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] != records::module(rec).name[..] {
            return E_FORMAT;
        }
        let (off, len) = match table_span(file) {
            Some(s) => s,
            None => return E_FORMAT,
        };
        if len > TABLE_MAX {
            return E_TOO_BIG;
        }
        match (Keys::parse(&self.tables[k][..self.table_len[k]]), Keys::parse(&file[off..off + len])) {
            (Ok(old), Ok(new)) => {
                if old.first_missing_key(&new).is_some() {
                    return E_RELOAD_KEYS;
                }
            }
            (_, Err(_)) => return E_FORMAT,
            (Err(_), _) => return E_NOT_FOUND,
        }
        self.capture(k, file);
        self.accepted[k] = true;
        // the services number the live set again: the live modules keep their numbers (sticky), a key the new file adds takes a free one
        let sel = live_settle_ctx(&self.tables, &self.table_len, &self.live_at(k));
        services().settle(&sel);
        OK
    }

    /// The services number the keys of `file` as if it were the module at selection index `k` (`settle` over the live set with its table there); the kept table of `k` is as it was after.
    #[inline(never)]
    unsafe fn number_keys(&mut self, k: usize, file: &[u8]) {
        let saved = (self.tables[k], self.table_len[k]);
        self.capture(k, file);
        let sel = live_settle_ctx(&self.tables, &self.table_len, &self.live_at(k));
        services().settle(&sel);
        self.tables[k] = saved.0;
        self.table_len[k] = saved.1;
    }

    /// The checks of a load of the file at `path` into selection index `k` (`boot::live_index`), before anything is loaded: nothing changes but the staging buffer. The file
    /// passes every check of a load (`check_file`, as a free record), no loaded module has its name (`E_LOADED`), and the planner accepts the live set with the file at `k`
    /// (`caps`: `live_caps` with `own = (0, 0)`, since nothing is given back). The services number the file's keys first, as a reload's check does, because the checks of the
    /// DSP section read the numbers by the selection index; the kept table of the index is as it was after. `OK`, or the refusal.
    pub unsafe fn live_check_load(&mut self, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        if k >= SEL_MAX {
            return E_SEL_COUNT;
        }
        if let Ok(staged) = self.m.stage(path) {
            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
            if table_of_file(file).is_some() {
                self.number_keys(k, file);
            }
        }
        boot_idx::with(k as u32, || {
            self.missing = [0; NEEDS_LEN];
            self.live_check_load_inner(k, path, caps)
        })
    }

    unsafe fn live_check_load_inner(&mut self, k: usize, path: *const u8, caps: &[DkrtCap]) -> i32 {
        // the plan first, as the boot does (it plans every file before it loads one): a service that no loaded provider offers is `E_MISSING` before the file's imports
        // are looked up, where it would be `E_IMPORT` for the first export of that service. A file whose table cannot be read goes on to `check_file`, which says why.
        if let Ok(staged) = self.m.stage(path) {
            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
            // a loaded name is its own answer: its keys would clash with the loaded module's, which the plan would call `E_PLAN_KEY`
            if file.len() >= H_NAME + dkneeds::NAME_LEN as usize {
                let mut rec = 0;
                while rec < MAX_MODULES {
                    if records::module(rec).used != 0 && records::module(rec).name[..] == file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] {
                        return E_LOADED;
                    }
                    rec += 1;
                }
            }
            if let Some(table) = table_of_file(file) {
                if Keys::parse(table).is_ok() {
                    // the search stages the provider files, so the module's file is staged again after it
                    let r = self.live_find(table);
                    if r != OK {
                        // a busy store: nothing is known about the card, the job asks again later (not "no such service")
                        return r;
                    }
                    match self.m.stage(path) {
                        Ok(staged) => {
                            let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
                            if let Some(table) = table_of_file(file) {
                                let r = self.live_plan(k, table, caps);
                                if r != OK {
                                    return r;
                                }
                                // (the staged bytes are replaced by this look at the providers' files: `table` is not used after it)
                                if self.live_brings_up_overlay(table) {
                                    return E_BOOT_ONLY;
                                }
                            }
                        }
                        Err(e) => return e,
                    }
                }
            }
        }
        let mut c = Checked::empty();
        let mut r = self.check_file(path, Slot::Free, &mut c);
        if r == E_IMPORT && self.needs_service_down(self.import_name) {
            // an import of a service that is not up yet resolves when the manager brings the service up and links the module (`resolve_imports` with the module's needs); an
            // import that still has no owner then refuses the load, which gives back everything it took
            r = self.check_head(path, Slot::Free, &mut c);
            if r == OK {
                r = self.check_body(&mut c);
            }
        }
        if r != OK {
            return r;
        }
        let mut rec = 0;
        while rec < MAX_MODULES {
            if records::module(rec).used != 0 && cstr_eq(records::module(rec).name.as_ptr(), c.f.add(H_NAME)) {
                return E_LOADED;
            }
            rec += 1;
        }
        if c.reg_size as usize > TABLE_MAX {
            return E_TOO_BIG;
        }
        if Keys::parse(core::slice::from_raw_parts(c.f.add(c.reg_off as usize), c.reg_size as usize)).is_err() {
            return E_FORMAT;
        }
        OK
    }

    /// The part of `live_load` before the file loads (see `reload_prepare`): the file's header table kept at index `k`, the services' numbering.
    pub(crate) unsafe fn live_prepare(&mut self, k: usize, path: *const u8) -> i32 {
        if k >= SEL_MAX {
            return E_SEL_COUNT;
        }
        // the file can have changed since `live_check_load`: the rules of the check hold for the bytes that load
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
        let (off, len) = match table_span(file) {
            Some(s) => s,
            None => return E_FORMAT,
        };
        if len > TABLE_MAX {
            return E_TOO_BIG;
        }
        if Keys::parse(&file[off..off + len]).is_err() {
            return E_FORMAT;
        }
        self.capture(k, file);
        self.accepted[k] = true;
        let sel = live_settle_ctx(&self.tables, &self.table_len, &self.live_at(k));
        services().settle(&sel);
        OK
    }

    /// Selection index `k` has no module any more (an unload, or a load that failed): the plan's acceptance goes, and no free record keeps the index. The kept header table
    /// stays (nothing reads it without a live record), and so do the services' sticky numbers, so a later load of the same file gets the same numbers.
    pub unsafe fn retire_index(&mut self, k: usize) {
        if k >= SEL_MAX {
            return;
        }
        self.accepted[k] = false;
        let mut rec = 0;
        while rec < MAX_MODULES {
            if self.sel_of(rec) as usize == k && records::module(rec).used == 0 {
                self.sel_of_rec[rec] = 0;
            }
            rec += 1;
        }
    }
}

/// The checks of a reload of module record `rec` by the file at `path`, before anything is unloaded (`Loader::reload_check`): the file, its keys, and the plan of
/// the live set with the file in the old module's slot. `OK` or the refusal.
pub unsafe fn reload_check(rec: usize, path: *const u8) -> i32 {
    let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
    let k = live_capacity(&mut caps, own_use(rec), dkrt_capacity_into);
    loader().reload_check(rec, path, &caps[..k])
}

/// The capacity as a plan of the whole live set needs it (the platform's capacity from `source` into `caps` through `live_caps`, with `own` given back); its count.
/// The host tests give the capacity of the device they boot as `source`.
pub(crate) unsafe fn live_capacity(caps: &mut [DkrtCap; CAP_MAX], own: (u32, u32), source: impl FnOnce(&mut [DkrtCap; CAP_MAX]) -> usize) -> usize {
    let k = source(caps);
    live_caps(&mut caps[..k], own);
    k
}

/// The checks of a load of the file at `path` into selection index `k` (`Loader::live_check_load`).
pub unsafe fn live_check_load(k: usize, path: *const u8) -> i32 {
    let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; CAP_MAX];
    let n = live_capacity(&mut caps, (0, 0), dkrt_capacity_into);
    loader().live_check_load(k, path, &caps[..n])
}

/// Selection index `k` has no module any more (`Loader::retire_index`).
pub unsafe fn retire_index(k: usize) {
    loader().retire_index(k)
}

/// What module record `rec` holds that its reload gives back before the new file loads: (arena bytes of its image block and its record block, DSP area bytes of its
/// range).
pub(crate) unsafe fn own_use(rec: usize) -> (u32, u32) {
    if rec >= MAX_MODULES {
        return (0, 0);
    }
    let block = |n: u32| if n == 0 { 0 } else { dkneeds::arena_block(n).unwrap_or(n) };
    let m = records::module(rec);
    (block(m.size) + block(crate::package::collect::block_of(rec).1), dsp::ds().mod_len[rec])
}

/// The capacity as a plan of the whole live set needs it: what the live modules use is counted by the plan again, so the counts they own go back to 0. The
/// physical resources (arena, staging buffer, DSP area, store, settings) keep their use, less `own` (the arena and DSP bytes of the module being reloaded, which its
/// teardown frees before the new file loads, so a large module is not refused for counting itself twice). The plan still counts the record block of every other live
/// module again, which errs on the side of refusing.
pub(crate) fn live_caps(caps: &mut [DkrtCap], own: (u32, u32)) {
    use dkrt_sdk::proto::*;
    let mut c = 0;
    while c < caps.len() {
        let kind = caps[c].kind;
        let physical = kind == CAPK_ARENA as u32 || kind == CAPK_STAGE as u32 || kind == CAPK_DSP_AREA as u32 || kind == CAPK_STORE as u32 || kind == CAPK_SETTINGS as u32;
        if kind == CAPK_ARENA as u32 {
            caps[c].used = caps[c].used.saturating_sub(own.0);
        } else if kind == CAPK_DSP_AREA as u32 {
            caps[c].used = caps[c].used.saturating_sub(own.1);
        }
        if !physical {
            caps[c].used = 0;
            caps[c].largest = if kind == CAPK_SELECTION as u32 { caps[c].limit } else { 0 };
        }
        c += 1;
    }
}

/// The selection indices at which a module is loaded.
pub(crate) unsafe fn live_indices() -> [bool; SEL_MAX] {
    loader().live_at(SEL_MAX)
}

/// Does the module or service file `file` list `name` among its imports?
fn imports_name(file: &[u8], name: &[u8]) -> bool {
    let be = |p: usize| file.get(p..p + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let (Some(off), Some(n)) = (be(dkneeds::H_IMPORT_OFF as usize), be(dkneeds::H_IMPORT_N as usize)) else { return false };
    let len = dkneeds::CF_IMPORT_LEN as usize;
    let mut i = 0;
    while i < n {
        let Some(e) = file.get(off + i * len..off + (i + 1) * len) else { return false };
        let end = e.iter().position(|b| *b == 0).unwrap_or(len);
        if &e[..end] == name {
            return true;
        }
        i += 1;
    }
    false
}
