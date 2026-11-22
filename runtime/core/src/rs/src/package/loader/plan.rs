//! The boot plan over the header tables of the selection: capture, provider search, admission and the numbering of keys.
use super::*;

impl<P: Platform, M: Images, R: Registry> Loader<P, M, R> {
    /// Keep the header table of the `.dkm` `file` (the whole file in the staging buffer) for selection index `k`.
    pub unsafe fn capture(&mut self, k: usize, file: &[u8]) {
        if k >= SEL_MAX || file.len() < dkneeds::HEADER as usize {
            return;
        }
        self.log_name[k] = name_part(&file[H_NAME..H_NAME + dkneeds::NAME_LEN as usize]);
        let be = |at: usize| u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
        let (off, len) = (be(dkneeds::keys::H_KEYS_OFF as usize), be(dkneeds::keys::H_KEYS_SIZE as usize));
        if len == 0 || len > TABLE_MAX || off > file.len() || len > file.len() - off {
            return;
        }
        self.tables[k][..len].copy_from_slice(&file[off..off + len]);
        self.table_len[k] = len;
    }

    /// The keys of record kind `kind` in the header table of the accepted module at selection index `at` (`CoreApi::keys_of`).
    pub fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        let k = at as usize;
        if k >= SEL_MAX || !self.accepted[k] {
            return 0;
        }
        dkrt_abi::keys::keys_of(&self.tables[k][..self.table_len[k]], kind, out)
    }

    /// Forget the tables (the start of a boot's planning).
    pub fn reset_plan(&mut self) {
        self.table_len = [0; SEL_MAX];
        self.accepted = [false; SEL_MAX];
        self.planned_dsp = (0, 0);
    }

    /// Search the card for the services the selection `count` modules long needs, from the tables `capture` kept. The hard needs that the runtime does not meet are looked up as
    /// `services/<name>.dks` (`discover::find`, one listing, only when there is such a need); the providers found are the package graph's services, and the manager brings
    /// them up before the modules that need them.
    #[inline(never)]
    pub unsafe fn find_providers(&mut self, count: usize) {
        let n = count.min(SEL_MAX);
        let mut mods: [Option<Keys>; SEL_MAX] = [None; SEL_MAX];
        let mut i = 0;
        while i < n {
            if self.table_len[i] != 0 {
                mods[i] = Keys::parse(&self.tables[i][..self.table_len[i]]).ok();
            }
            i += 1;
        }
        // the hard needs that no service of the runtime meets
        self.found.n = 0;
        let mut want = [0u32; WANT_MAX];
        let nw = unmet_needs(&mods[..n], &mut want);
        if nw != 0 {
            discover::find(&mut self.m, &services(), &want[..nw], dkrt_abi::fw_id::image_fw_id(&self.p), &mut self.found);
            // a limit of the search was passed: a need may be unmet for that reason, not for a missing file; each cause has its own name word (`(tab`, `(fil`, `(nee`) and its `TRUNC_*` bit as the hash word, which `loadFailureText` of runtime-log.ts turns into text
            let t = self.found.truncated;
            if t & discover::TRUNC_TABLE != 0 {
                crate::boot::report_provider(b"(tab", E_PLAN_FIT, discover::TRUNC_TABLE as u32, 0);
            }
            if t & discover::TRUNC_FILES != 0 {
                crate::boot::report_provider(b"(fil", E_PLAN_FIT, discover::TRUNC_FILES as u32, 0);
            }
            if t & discover::TRUNC_NEEDS != 0 {
                crate::boot::report_provider(b"(nee", E_PLAN_FIT, discover::TRUNC_NEEDS as u32, 0);
            }
        }
    }

    /// The judgment of package `id` by the rules of the core (`Admission`): a module against the providers and the modules admitted before it, a provider against the providers.
    /// The DSP load the admitted packages left is what `dkrt_capacity` reports (`planned_dsp`).
    pub(crate) unsafe fn admit_package(&mut self, adm: &mut Admission, id: PackageId) -> Verdict {
        let mut mods: [Option<Keys>; SEL_MAX] = [None; SEL_MAX];
        let mut i = 0;
        while i < SEL_MAX {
            if self.table_len[i] != 0 {
                mods[i] = Keys::parse(&self.tables[i][..self.table_len[i]]).ok();
            }
            i += 1;
        }
        let provs = parse_provs(&self.found, self.found.n);
        let rec = id.0 as usize;
        let v = if rec < MAX_MODULES {
            adm.judge_module(&services(), &mods, &provs[..self.found.n.min(PROVIDER_MAX)], rec)
        } else {
            adm.judge_provider(&services(), &provs[..self.found.n.min(PROVIDER_MAX)], rec - MAX_MODULES)
        };
        self.planned_dsp = adm.dsp_load();
        v
    }

    /// The services plan what they keep per selected module, once every service of the boot has been tried: `admitted[k]` says that the module at selection index `k` was admitted and
    /// every service it needs is up, so a module that is refused (a repeated key, no room, a missing need) takes no number and a key that it repeats never reaches a service's
    /// assignment. `keys_of` gives the keys of such a module only.
    #[inline(never)]
    pub unsafe fn number_boot_keys(&mut self, admitted: &[bool; SEL_MAX], count: usize) {
        let n = count.min(SEL_MAX);
        let mut a = 0;
        while a < SEL_MAX {
            self.accepted[a] = a < n && admitted[a];
            a += 1;
        }
        let sel = settle_ctx(&self.accepted, &self.tables, &self.table_len, n);
        services().settle(&sel);
    }

}

/// Keep the header table of the `.dkm` `file` (the whole file in the staging buffer) for selection index `k`.
pub unsafe fn capture(k: usize, file: &[u8]) {
    loader().capture(k, file)
}

/// The keys of record kind `kind` of the accepted module at selection index `at` (`CoreApi::keys_of`). A read of the loader's tables, which `settle` and the loads that
/// follow do not change.
pub fn keys_of(at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
    loader().keys_of(at, kind, out)
}

/// Forget the tables (the start of a boot's planning).
pub unsafe fn reset_plan() {
    loader().reset_plan()
}

/// The capacity as the boot's admission starts with it (`dkrt_capacity`, into `caps`; its count). The report counts the selection's paths as used from the start of the boot
/// (`boot::selected`), and the admission counts them one by one as it takes the modules; and the admission starts with no DSP load (the report counts its own result from its end).
pub(crate) unsafe fn boot_caps(caps: &mut [DkrtCap; CAP_MAX]) -> usize {
    let k = crate::package::capacity::dkrt_capacity(caps.as_mut_ptr(), CAP_MAX as u32) as usize;
    let mut c = 0;
    while c < k {
        if caps[c].kind == dkrt_sdk::proto::CAPK_SELECTION as u32 {
            caps[c].used = 0;
            caps[c].largest = caps[c].limit;
        }
        if caps[c].kind == dkrt_sdk::proto::CAPK_DSP_COST as u32 {
            caps[c].used = 0;
            caps[c].largest = 0;
        }
        c += 1;
    }
    k
}

/// Search the card for the services the selection `count` modules long needs, from the tables `capture` kept (`Loader::find_providers`).
pub unsafe fn find_providers(count: usize) {
    loader().find_providers(count)
}

/// The DSP load of the last plan: (load, largest per-voice cost). A read of the field that takes no reference: module code can reach it (`dkrt_capacity`) while
/// a load runs.
pub unsafe fn planned_dsp() -> (u32, u32) {
    *addr_of!(LOADER.planned_dsp)
}


/// A host test's boot: the module at selection index `at` has the header table `table` and the plan accepted it.
#[cfg(test)]
pub(crate) fn test_accept(at: usize, table: &[u8]) {
    let l = loader();
    l.tables[at][..table.len()].copy_from_slice(table);
    l.table_len[at] = table.len();
    l.accepted[at] = true;
}

/// What `number_keys` hands to the services: the keys of the modules in `accepted`. A module that was refused gives none.
#[inline(never)]
pub(crate) fn settle_ctx(accepted: &[bool; SEL_MAX], tables: &[[u8; TABLE_MAX]; SEL_MAX], lens: &[usize; SEL_MAX], n: usize) -> dkrt_abi::service::SettleCtx {
    let mut sel = dkrt_abi::service::SettleCtx::new();
    sel.count = n;
    let mut i = 0;
    while i < n {
        if accepted[i] {
            sel.nkeys[i] = table_keys(&tables[i][..lens[i]], &mut sel.keys[i]) as u8;
        }
        i += 1;
    }
    sel
}

/// The header table of a `.dkm`: its offset and length in `file`, `None` when the header is short, the table empty or outside the file.
pub(super) fn table_span(file: &[u8]) -> Option<(usize, usize)> {
    if file.len() < dkneeds::HEADER as usize {
        return None;
    }
    let be = |at: usize| u32::from_be_bytes([file[at], file[at + 1], file[at + 2], file[at + 3]]) as usize;
    let (off, len) = (be(dkneeds::keys::H_KEYS_OFF as usize), be(dkneeds::keys::H_KEYS_SIZE as usize));
    if len == 0 || off > file.len() || len > file.len() - off {
        None
    } else {
        Some((off, len))
    }
}

/// The header table of `file` when it fits the planner's limit.
pub(super) fn table_of_file(file: &[u8]) -> Option<&[u8]> {
    let (off, len) = table_span(file)?;
    if len > TABLE_MAX {
        None
    } else {
        Some(&file[off..off + len])
    }
}

/// What a reload's `settle` hands to the services: the keys of the modules in `live` (selection indices), in selection order. The count reaches the last live one.
#[inline(never)]
pub(crate) fn live_settle_ctx(tables: &[[u8; TABLE_MAX]; SEL_MAX], lens: &[usize; SEL_MAX], live: &[bool; SEL_MAX]) -> dkrt_abi::service::SettleCtx {
    let mut sel = dkrt_abi::service::SettleCtx::new();
    let mut i = 0;
    while i < SEL_MAX {
        if live[i] {
            sel.count = i + 1;
            sel.nkeys[i] = table_keys(&tables[i][..lens[i]], &mut sel.keys[i]) as u8;
        }
        i += 1;
    }
    sel
}

/// The keys of a module's header table that `settle` hands to the services (`table` empty: none), at most 8; their count.
#[inline(never)]
pub(crate) fn table_keys(table: &[u8], out: &mut [u32; 8]) -> usize {
    let k = match dkrt_abi::keys::KeysCopy::from_bytes(table) {
        Ok(k) => k,
        Err(_) => return 0,
    };
    let n = k.nm.min(8);
    out[..n].copy_from_slice(&k.mk[..n]);
    n
}

/// The header tables of the first `n` providers the search found.
#[inline(never)]
pub(super) fn service_import_owner(keys: Keys<'_>, name: u32) -> Result<Option<(u32, u32)>, i32> {
    use dkneeds::keys::tag;
    let mut i = 0;
    while let Some((hash, service, _)) = keys.words(tag::SERVICE_IMPORTS, i) {
        if hash == name {
            let mut j = 0;
            while let Some((needed, version, flags)) = keys.words(tag::NEEDS, j) {
                if needed == service && version != 0 && flags & 1 != 0 {
                    return Ok(Some((service, version)));
                }
                j += 1;
            }
            return Err(E_IMPORT);
        }
        i += 1;
    }
    Ok(None)
}

pub(super) fn parse_provs(found: &Found, n: usize) -> [Option<Keys<'_>>; PROVIDER_MAX] {
    let mut provs: [Option<Keys>; PROVIDER_MAX] = [None; PROVIDER_MAX];
    let mut j = 0;
    while j < n.min(PROVIDER_MAX) {
        provs[j] = Keys::parse(found.table(j)).ok();
        j += 1;
    }
    provs
}

/// Most hard needs the search is asked for: one boot's distinct needs of services that the runtime does not provide.
pub(super) const WANT_MAX: usize = 24;

/// The distinct hard needs of `mods` that no service of the runtime meets, into `out`; their count.
#[inline(never)]
pub(super) fn unmet_needs(mods: &[Option<Keys>], out: &mut [u32; WANT_MAX]) -> usize {
    let mut n = 0;
    let mut m = 0;
    while m < mods.len() {
        if let Some(t) = &mods[m] {
            let mut i = 0;
            while let Some(w) = t.words(dkneeds::keys::tag::NEEDS, i) {
                i += 1;
                if w.2 & 1 == 0 || discover::is_runtime_service(w.0, &services()) {
                    continue;
                }
                let mut seen = false;
                let mut k = 0;
                while k < n {
                    seen |= out[k] == w.0;
                    k += 1;
                }
                if !seen && n < WANT_MAX {
                    out[n] = w.0;
                    n += 1;
                }
            }
        }
        m += 1;
    }
    n
}
