//! A copy of the header table of a `.dkm` that the loader keeps for the commit, and what a service may read of it (`ModuleCommitCtx::keys`): the `[uses]` row counts, the machine
//! keys and effect ids (the keys table), and the declarations of a service file.
use crate::service::Declare;
use dkneeds::MAX_RECORDS;
// The key groups of the header table (the .dkm format, `dkneeds::keys`), not record kinds: the core reads the table, whoever uses its keys.
use dkneeds::{KIND_EFFECT, KIND_LFO_SHAPE, KIND_MACHINE};
use dkrt_sdk::{E_CONTRACT, E_FORMAT};

/// A copy of the keys table (the staged file is overwritten by anything `init` reads from the store).
#[derive(Clone, Copy)]
pub struct KeysCopy {
    pub machine_rows: u32,
    pub param_rows: u32,
    pub page_rows: u32,
    pub nm: usize,
    pub nf: usize,
    /// the number of LFO shape keys, and the keys (`hash24 << 8`)
    pub ns: usize,
    pub sk: [u32; MAX_RECORDS as usize],
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
            ns: 0,
            sk: [0; MAX_RECORDS as usize],
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
                } else if kind == KIND_LFO_SHAPE {
                    if k.ns >= MAX_RECORDS as usize {
                        return Err(E_FORMAT);
                    }
                    k.sk[k.ns] = v;
                    k.ns += 1;
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
