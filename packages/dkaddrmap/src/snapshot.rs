//! The load table from raw memory: the bytes of the runtime's module table (`DKRT_STATE`), its DSP placement table (`DKRT_DSP`) and the
//! `dyndsp_info` table of the DSP loader, as a host reads them from an emulator or a device. One decoder for every host (the workbench uses it
//! through wasm). The layout of the records is that of `State` (modules/core/runtime/src/rs/src/lib.rs) and `DspState` (.../dsp.rs).
//!
//! The constants of the DSP placement come from the map header ([`Loader`], written by `patchkit apply`), not from the caller.

use crate::{Load, LoadTable};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// `State`: ten header words, then one record per module slot.
const STATE_HEADER: usize = 40;
/// One module record: used, name[32], version, base, size, fini (13 words).
const STATE_RECORD: usize = 52;
/// `DspState`: 13 words, then `mod_off[slots]` and `mod_len[slots]` (the byte offset of a module's range in the area; length 0: no DSP part).
const DSP_HEADER: usize = 52;
/// `dyndsp_info`: word 3 is the area, an L2 byte address.
const INFO_AREA: usize = 12;
const INFO_BYTES: usize = 16;

/// The symbols read for the load table, as in the patch plan.
pub const SYM_STATE: &str = "DKRT_STATE";
pub const SYM_DSP: &str = "DKRT_DSP";
pub const SYM_INFO: &str = "dyndsp_info";

/// What a host needs to read the load table of the image a map belongs to (key of a ship map).
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Loader {
    /// Module slots of the runtime's tables.
    pub modules: u64,
    /// L2 memory: the byte address of the nominal base, and the software PC of the same place (`pc = sw + (byte - byte_base) / 2`).
    pub l2_byte_base: u64,
    pub l2_sw_base: u64,
    /// The image has the run-time DSP loader (`DKRT_DSP`, `dyndsp_info`).
    #[serde(default)]
    pub dsp: bool,
}

/// One range of memory to read: a symbol of the patch plan and the bytes from its address.
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct Read {
    pub symbol: &'static str,
    pub len: u64,
}

/// Raw bytes by symbol name.
pub type Snapshot = BTreeMap<String, Vec<u8>>;

impl Loader {
    /// The ranges to read, in the order of [`decode_loads`] needs them.
    pub fn reads(&self) -> Vec<Read> {
        let n = self.modules;
        let mut r = vec![Read {
            symbol: SYM_STATE,
            len: STATE_HEADER as u64 + STATE_RECORD as u64 * n,
        }];
        if self.dsp {
            r.push(Read {
                symbol: SYM_DSP,
                len: DSP_HEADER as u64 + 8 * n,
            });
            r.push(Read {
                symbol: SYM_INFO,
                len: INFO_BYTES as u64,
            });
        }
        r
    }
}

fn word(b: &[u8], at: usize) -> u64 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as u64
}

/// The bytes of a hex string.
pub fn parse_hex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return Err("hex: an even number of hex digits".into());
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| "hex: bad digit".to_string()))
        .collect()
}

/// The load table of a snapshot. A module record in use becomes a [`Load`]; with the DSP loader in the snapshot, a module whose range has a
/// length also gets `dsp_base`, the software PC where its DSP image starts (the slot index is the index in `DKRT_DSP`).
pub fn decode_loads(loader: &Loader, snap: &Snapshot) -> Result<LoadTable, String> {
    let state = snap
        .get(SYM_STATE)
        .ok_or_else(|| format!("snapshot has no {SYM_STATE}"))?;
    if state.len() < STATE_HEADER || (state.len() - STATE_HEADER) % STATE_RECORD != 0 {
        return Err(format!(
            "{SYM_STATE}: {} bytes are not a header and whole records",
            state.len()
        ));
    }
    let slots = (state.len() - STATE_HEADER) / STATE_RECORD;
    let dsp = match (snap.get(SYM_DSP), snap.get(SYM_INFO)) {
        (Some(d), Some(i)) => {
            if d.len() < DSP_HEADER + 8 * slots || i.len() < INFO_BYTES {
                return Err(format!(
                    "{SYM_DSP} or {SYM_INFO} too short for {slots} slots"
                ));
            }
            Some((d, word(i, INFO_AREA)))
        }
        _ => None,
    };
    let mut modules = vec![];
    for slot in 0..slots {
        let at = STATE_HEADER + slot * STATE_RECORD;
        if word(state, at) == 0 {
            continue;
        }
        let raw = &state[at + 4..at + 36];
        let id = String::from_utf8_lossy(&raw[..raw.iter().position(|&b| b == 0).unwrap_or(32)])
            .into_owned();
        let mut load = Load {
            id,
            base: word(state, at + 40),
            size: word(state, at + 44),
            dsp_base: None,
            fini: Some(word(state, at + 48)).filter(|&f| f != 0),
        };
        if let Some((d, area)) = dsp {
            let off = word(d, DSP_HEADER + 4 * slot);
            let len = word(d, DSP_HEADER + 4 * (slots + slot));
            let byte = area + off;
            if len != 0 && area != 0 && byte >= loader.l2_byte_base {
                load.dsp_base = Some(loader.l2_sw_base + (byte - loader.l2_byte_base) / 2);
            }
        }
        modules.push(load);
    }
    Ok(LoadTable { modules })
}
