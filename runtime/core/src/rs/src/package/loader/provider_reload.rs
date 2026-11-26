//! The checks of a service reload before anything changes (`Loader::provider_check`, `Loader::provider_prepare`): the new file of a loaded service is held to what its consumers
//! already rely on, because the closure reload takes the consumers down first and a refusal after that leaves them down.
use super::*;
use super::live::imports_name;
use core::mem::MaybeUninit;
use dkrt_lifecycle::{LiveSet, PackageGraph};

/// Tables of the scratch graph: the live modules and the providers the search found.
const GRAPH_MAX: usize = SEL_MAX + PROVIDER_MAX;

impl<P: PackagePlatform, M: Images, R: Registry> Loader<P, M, R> {
    /// The provider slot of record `rec` when it is a loaded service the search found, else `None`.
    unsafe fn provider_slot(&self, rec: usize) -> Option<usize> {
        let j = rec.checked_sub(MAX_MODULES)?;
        // the port file is no service of a consumer's: a restart changes it
        (j < self.found.n.min(PROVIDER_MAX) && rec < records::REC_MAX && records::module(rec).used != 0 && self.roles[rec] == 1).then_some(j)
    }

    /// The checks of a reload of service record `rec` by the file at `path`, before anything is torn down: nothing changes but the staging buffer. First the rules of the file as
    /// the consumers see it (`provider_rules`: they come first, as the plan's answer comes before a load's imports), then every check of a service load (`check_file`, with the old
    /// record's sites counted as stock, `Installer::check_replacing`). `OK`, or the refusal.
    pub unsafe fn provider_check(&mut self, rec: usize, path: *const u8) -> i32 {
        let Some(j) = self.provider_slot(rec) else {
            return E_NOT_FOUND;
        };
        match self.m.stage(path) {
            Ok(s) => {
                let file = core::slice::from_raw_parts(s.ptr, s.size as usize);
                if let Err(e) = self.provider_rules(j, rec, file) {
                    return e;
                }
            }
            Err(e) => return e,
        }
        self.replacing = Some(rec as u8);
        let mut c = Checked::empty();
        let r = boot_idx::with(PROVIDER_IDX, || self.check_file(path, Slot::Provider(rec), &mut c));
        self.replacing = None;
        r
    }

    /// The part of the reload before the new file loads: the rules of the check for the bytes that load (the card file can change while the consumers drain), and the file's
    /// header table kept as provider `j`'s table, which the package graph reads when the manager brings the set back. The manager loads the file after it.
    pub(crate) unsafe fn provider_prepare(&mut self, rec: usize, path: *const u8) -> i32 {
        let Some(j) = self.provider_slot(rec) else {
            return E_NOT_FOUND;
        };
        let staged = match self.m.stage(path) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let file = core::slice::from_raw_parts(staged.ptr, staged.size as usize);
        let (off, len) = match self.provider_rules(j, rec, file) {
            Ok(s) => s,
            Err(e) => return e,
        };
        self.found.tables[j][..len].copy_from_slice(&file[off..off + len]);
        self.found.table_len[j] = len;
        OK
    }

    /// The rules the new file `file` of service record `rec` (provider `j`) meets: the same name; no DSP part (`E_BOOT_ONLY`: the closure reload has no DSP step); every key and every export of the old table still there (a consumer's file
    /// imports them by name); no import of the settings overlay (a service that keeps per-project state belongs to the boot's set, `E_BOOT_ONLY`); and every live package's needs
    /// still met by the providers with the new table in place (`E_MISSING`, `E_CONTRACT`: a consumer that needs an older major than the file offers). The span of the file's
    /// header table.
    pub(super) unsafe fn provider_rules(&self, j: usize, rec: usize, file: &[u8]) -> Result<(usize, usize), i32> {
        if file.len() < H_NAME + dkneeds::NAME_LEN as usize || file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize] != records::module(rec).name[..] {
            return Err(E_FORMAT); // another name is another service
        }
        let (off, len) = table_span(file).ok_or(E_FORMAT)?;
        if file.len() >= dkneeds::HEADER as usize && rd32(file.as_ptr().add(H_DSP_SIZE)) != 0 {
            return Err(E_BOOT_ONLY);
        }
        if len > TABLE_MAX {
            return Err(E_TOO_BIG);
        }
        let new = Keys::parse(&file[off..off + len]).map_err(|_| E_FORMAT)?;
        let old = Keys::parse(self.found.table(j)).map_err(|_| E_NOT_FOUND)?;
        let mut w = 0;
        let mut dropped = old.first_missing_key(&new).is_some();
        while let Some(e) = old.words(dkneeds::keys::tag::EXPORTS, w) {
            let mut kept = false;
            let mut i = 0;
            while let Some(n) = new.words(dkneeds::keys::tag::EXPORTS, i) {
                kept |= n.0 == e.0;
                i += 1;
            }
            dropped |= !kept;
            w += 1;
        }
        if dropped {
            return Err(E_RELOAD_KEYS);
        }
        if imports_name(file, b"dkc_settings_overlay_set") {
            return Err(E_BOOT_ONLY);
        }
        match self.provider_needs(j, &file[off..off + len]) {
            OK => Ok((off, len)),
            e => Err(e),
        }
    }

    /// The needs of every live package, as the package manager would meet them with the table `new` for provider `j`: the live modules (by selection index) and the providers the
    /// search found (the others as they are, `j` with `new`), in a graph of their own, every live package a root. `OK`, or the planner's code for the first need nothing meets
    /// or that is met at an older major.
    unsafe fn provider_needs(&self, j: usize, new: &[u8]) -> i32 {
        let mut tables: [(PackageId, &[u8]); GRAPH_MAX] = [(PackageId(0), &[]); GRAPH_MAX];
        let mut roots = [PackageId(0); GRAPH_MAX];
        let (mut n, mut nr) = (0, 0);
        let mut r = 0;
        while r < MAX_MODULES {
            let at = self.sel_of(r) as usize;
            if records::module(r).used != 0 && at < SEL_MAX && self.table_len[at] != 0 && n < GRAPH_MAX {
                tables[n] = (PackageId(at as u8), &self.tables[at][..self.table_len[at]]);
                roots[nr] = PackageId(at as u8);
                n += 1;
                nr += 1;
            }
            r += 1;
        }
        let mut i = 0;
        while i < self.found.n.min(PROVIDER_MAX) {
            let usable = i == j || (!self.unusable[i] && Keys::parse(self.found.table(i)).is_ok());
            if usable && n < GRAPH_MAX {
                let id = PackageId((MAX_MODULES + i) as u8);
                tables[n] = (id, if i == j { new } else { self.found.table(i) });
                if i == j || records::module(MAX_MODULES + i).used != 0 {
                    roots[nr] = id;
                    nr += 1;
                }
                n += 1;
            }
            i += 1;
        }
        let core = crate::package::lifecycle::CoreApis::new();
        let mut g = MaybeUninit::<PackageGraph>::uninit();
        if let Err(e) = PackageGraph::init_from_tables(g.as_mut_ptr(), &tables[..n], core.as_slice()) {
            return crate::package::lifecycle::code_of(&dkrt_lifecycle::LifecycleError::Graph(e));
        }
        match (*g.as_ptr()).bring_up_order(&roots[..nr], &LiveSet::new()) {
            Ok(_) => OK,
            Err(e) => crate::package::lifecycle::code_of(&dkrt_lifecycle::LifecycleError::Graph(e)),
        }
    }
}

/// A service reload's check (`Loader::provider_check`): `OK` when nothing refuses the file, else the refusal; nothing changes but the staging buffer.
pub unsafe fn provider_check(rec: usize, path: *const u8) -> i32 {
    loader().provider_check(rec, path)
}
