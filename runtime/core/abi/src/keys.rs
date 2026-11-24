//! A copy of the header table of a `.dkm` that the loader keeps for the commit, and what a service may read of it (`ModuleCommitCtx::keys`): the `[uses]` row counts, the machine
//! keys and effect ids (the keys table), and the declarations of a service file.
use crate::service::Declare;
use dkneeds::MAX_RECORDS;
// The key groups of the header table (the .dkm format, `dkneeds::keys`), not record kinds: the core reads the table, whoever uses its keys.
use dkneeds::{KIND_EFFECT, KIND_MACHINE};
use dkrt_sdk::{E_CONTRACT, E_FORMAT};

/// A copy of the keys table (the staged file is overwritten by anything `init` reads from the store).
#[derive(Clone, Copy)]
pub struct KeysCopy {
    pub machine_rows: u32,
    pub param_rows: u32,
    pub page_rows: u32,
    pub nm: usize,
    pub nf: usize,
    pub mk: [u32; MAX_RECORDS as usize],
    pub fk: [u32; MAX_RECORDS as usize],
    /// the KINDS, SLOTS, HOOKS, EXPORTS and RESOURCES entries of the table, as `init`'s declarations must repeat them (`decl_matches`)
    pub decl: Declare,
    /// The HOOKS items of other devices (name hash, entry offset), set aside by `set_aside_foreign_hooks`: a service declares every hook it has, plain, and the commit leaves out
    /// the declarations that the table lists for another device only.
    pub foreign: [(u32, u32); KeysCopy::FOREIGN_MAX],
    pub nforeign: usize,
}

impl KeysCopy {
    /// Most hook items of other devices kept for the comparison; more are not kept, and a declaration of such a hook is then refused like any other mismatch.
    pub const FOREIGN_MAX: usize = 24;

    pub const fn empty() -> KeysCopy {
        KeysCopy {
            machine_rows: 0,
            param_rows: 0,
            page_rows: 0,
            nm: 0,
            nf: 0,
            mk: [0; MAX_RECORDS as usize],
            fk: [0; MAX_RECORDS as usize],
            decl: Declare::new(),
            foreign: [(0, 0); KeysCopy::FOREIGN_MAX],
            nforeign: 0,
        }
    }

    /// Take the HOOKS items that are not for the device with runtime id `device_id` out of `decl` and keep their (hash, entry offset) in `foreign`: the table of a service file lists
    /// the hooks of every device (one name may have an entry for each of disjoint devices), the loader works with the items of its own (the device mask dropped).
    pub fn set_aside_foreign_hooks(&mut self, device_id: u32) {
        let d = &mut self.decl;
        let (mut from, mut to) = (0, 0);
        while from < d.n {
            let it = d.items[from];
            from += 1;
            if it.tag == dkneeds::keys::tag::HOOKS && !dkneeds::keys::hook_for_device(it.b, device_id) {
                if self.nforeign < KeysCopy::FOREIGN_MAX {
                    self.foreign[self.nforeign] = (it.a, it.b & dkneeds::keys::HOOK_ENTRY_MASK);
                    self.nforeign += 1;
                }
                continue;
            }
            d.items[to] = it;
            if it.tag == dkneeds::keys::tag::HOOKS {
                // the items of this device keep the entry only
                d.items[to].b &= dkneeds::keys::HOOK_ENTRY_MASK;
            }
            to += 1;
        }
        d.n = to;
    }

    /// Whether the table lists the hook (`hash`, `entry`: the image offset, or `HOOK_ENTRY_PATCH`) for another device only.
    pub fn lists_for_other_device(&self, hash: u32, entry: u32) -> bool {
        let mut i = 0;
        while i < self.nforeign {
            if self.foreign[i] == (hash, entry) {
                return true;
            }
            i += 1;
        }
        false
    }

    /// Parse the header table bytes. A file with no table, or another magic or version, is the format of runtime 0.7: `E_CONTRACT`; a damaged
    /// table is `E_FORMAT`.
    pub fn from_bytes(b: &[u8]) -> Result<KeysCopy, i32> {
        let mut k = KeysCopy::empty();
        let keys = dkneeds::keys::Keys::parse(b).map_err(|e| {
            if e == dkneeds::keys::KeysError::Format {
                E_CONTRACT
            } else {
                E_FORMAT
            }
        })?;
        k.machine_rows = keys.machine_rows();
        k.param_rows = keys.param_rows();
        k.page_rows = keys.page_rows();
        let mut g = 0;
        while let Some((kind, list)) = keys.group(g) {
            g += 1;
            let mut j = 0;
            while j < list.len() {
                let v = list.get(j);
                j += 1;
                if kind == KIND_MACHINE {
                    if k.nm >= MAX_RECORDS as usize {
                        return Err(E_FORMAT);
                    }
                    k.mk[k.nm] = v;
                    k.nm += 1;
                } else if kind == KIND_EFFECT {
                    if k.nf >= MAX_RECORDS as usize {
                        return Err(E_FORMAT);
                    }
                    k.fk[k.nf] = v;
                    k.nf += 1;
                }
            }
        }
        for tg in [
            dkneeds::keys::tag::KINDS,
            dkneeds::keys::tag::SLOTS,
            dkneeds::keys::tag::HOOKS,
            dkneeds::keys::tag::EXPORTS,
            dkneeds::keys::tag::RESOURCES,
        ] {
            let mut i = 0;
            while let Some(w) = keys.words(tg, i) {
                // a DSP slot fill is dispatch code of the file's DSP section, not a function `init` declares: the packer wrote it from `[[fill]] space = "dsp"`
                if !(tg == dkneeds::keys::tag::SLOTS && w.1 & dkneeds::keys::SLOT_DSP != 0) {
                    k.decl.add(tg, w.0, w.1, w.2);
                }
                i += 1;
            }
        }
        if k.decl.full {
            return Err(E_FORMAT);
        }
        Ok(k)
    }
}

/// The keys of group `kind` in a header table (`table` empty or damaged: none), at most 8, into `out`; their count. The core reads the table, whoever uses its
/// keys: a service asks for the group of its own kind (`CoreApi::keys_of`).
pub fn keys_of(table: &[u8], kind: u32, out: &mut [u32; 8]) -> usize {
    let keys = match dkneeds::keys::Keys::parse(table) {
        Ok(k) => k,
        Err(_) => return 0,
    };
    let mut n = 0;
    let mut g = 0;
    while let Some((k, list)) = keys.group(g) {
        g += 1;
        if k != kind {
            continue;
        }
        let mut j = 0;
        while j < list.len() && n < out.len() {
            out[n] = list.get(j);
            n += 1;
            j += 1;
        }
    }
    n
}
