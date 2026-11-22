//! The checks of a live job over the live set of modules, before anything changes (`Loader::reload_check`, `Loader::live_check_load`), and the admission state a live load
//! starts from (`Loader::seed_live`). The needs of the new file are asked of the package graph (`dkrt-lifecycle`), as the manager asks them when it loads the file; the rules of the
//! core (owners, keys, capacity) are `Admission`, over every live module with the new table at its index.
use super::*;
use crate::admission::Admission;
use dkrt_lifecycle::{LiveSet, PackageGraph};
use core::mem::MaybeUninit;

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
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
        let core = crate::lifecycle::CoreApis::new();
        let mut g = MaybeUninit::<PackageGraph>::uninit();
        if let Err(r) = PackageGraph::init_from_tables(g.as_mut_ptr(), &tables[..n], core.as_slice()) {
            return crate::lifecycle::code_of(&dkrt_lifecycle::LifecycleError::Graph(r));
        }
        match (*g.as_ptr()).bring_up_order(&[PackageId(k as u8)], &LiveSet::new()) {
            Ok(_) => OK,
            Err(r) => crate::lifecycle::code_of(&dkrt_lifecycle::LifecycleError::Graph(r)),
        }
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
