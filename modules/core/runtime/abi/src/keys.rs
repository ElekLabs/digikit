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
}

impl KeysCopy {
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
        }
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
