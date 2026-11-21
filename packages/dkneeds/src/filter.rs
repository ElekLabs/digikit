//! Version 6 extends the 64-byte DKDS header with a count and its filter records.
//! A record carries four big-endian words. The descriptor stays in the DSP image.
use crate::{
    inside, word, Bytes, Error, DSP_HEADER, DSP_H_BANK_OFF, DSP_H_FX_OFF, DSP_H_IMAGE_OFF,
    DSP_H_IMAGE_SIZE, DSP_H_IMPORT_OFF, DSP_H_RELOC_N, DSP_H_RELOC_OFF, DSP_H_VOICE_OFF,
    MAX_RELOCS, MAX_STATE_BYTES, RELOC_LEN,
};

pub const VERSION: u32 = 6;
pub const TABLE: u32 = DSP_HEADER + 4;
pub const RECORD: u32 = 16;
pub const MAX: u32 = 8;
pub const VOICES: u32 = 32;
pub use crate::gen_limits::filter as contract;
use contract::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    pub key: u32,
    pub coef: u32,
    pub process: u32,
    pub desc: u32,
}

impl Filter {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let w = |i: usize| u32::from_be_bytes(bytes[i..i + 4].try_into().unwrap());
        Self {
            key: w(0),
            coef: w(4),
            process: w(8),
            desc: w(12),
        }
    }
}

/// Check the table and descriptors before allocating state or following any callback.
pub fn check<B: Bytes>(src: &mut B, base: u32, size: u32) -> Result<(u32, u32), Error> {
    if !inside(base, size, src.size()) || size < TABLE {
        return Err(Error::Dsp);
    }
    let count = word(src, base + DSP_HEADER)?;
    if count == 0 || count > MAX || !inside(TABLE, count * RECORD, size) {
        return Err(Error::Dsp);
    }
    let end = TABLE + count * RECORD;
    for field in [
        DSP_H_IMAGE_OFF,
        DSP_H_IMPORT_OFF,
        DSP_H_RELOC_OFF,
        DSP_H_VOICE_OFF,
        DSP_H_FX_OFF,
    ] {
        if word(src, base + field)? < end {
            return Err(Error::Dsp);
        }
    }
    let bank = word(src, base + DSP_H_BANK_OFF)?;

    let image = word(src, base + DSP_H_IMAGE_OFF)?;
    let len = word(src, base + DSP_H_IMAGE_SIZE)?;
    if !inside(image, len, size) {
        return Err(Error::Dsp);
    }
    if bank != 0 && (bank < end || bank & 3 != 0 || !inside(bank, 32, image)) {
        return Err(Error::Dsp);
    }
    let reloc = word(src, base + DSP_H_RELOC_OFF)?;
    let nreloc = word(src, base + DSP_H_RELOC_N)?;
    if nreloc > MAX_RELOCS || !inside(reloc, nreloc * RELOC_LEN, size) {
        return Err(Error::Dsp);
    }
    let mut keys = [0; MAX as usize];
    let mut total = 0u32;
    for i in 0..count {
        let mut raw = [0; RECORD as usize];
        if !src.read(base + TABLE + i * RECORD, &mut raw) {
            return Err(Error::Unreadable);
        }
        let f = Filter::from_bytes(&raw);
        if f.key == 0
            || keys[..i as usize].contains(&f.key)
            || [f.coef, f.process]
                .iter()
                .any(|pc| pc & 1 != 0 || *pc >= len)
            || f.desc & 3 != 0
            || !inside(f.desc, DK_FILTER_DESC_BYTES, len)
        {
            return Err(Error::Dsp);
        }
        for j in 0..nreloc {
            let at = word(src, base + reloc + j * RELOC_LEN)?;
            if at < f.desc + DK_FILTER_DESC_BYTES && at.saturating_add(6) > f.desc {
                return Err(Error::Dsp);
            }
        }
        keys[i as usize] = f.key;
        let desc = base + image + f.desc;
        let state = word(src, desc + DK_FILTER_DESC_STATE_BYTES)?.swap_bytes();
        if word(src, desc)?.swap_bytes() != DK_FILTER_MAGIC
            || word(src, desc + DK_FILTER_DESC_CONTRACT)?.swap_bytes() != DK_FILTER_V1
            || word(src, desc + DK_FILTER_DESC_N_PARAMS)?.swap_bytes() > DK_FILTER_MAX_PARAMS
            || state & 3 != 0
            || state > MAX_STATE_BYTES
        {
            return Err(Error::Dsp);
        }
        total = total
            .checked_add(((state + 7) & !7) * VOICES)
            .ok_or(Error::Dsp)?;
    }
    Ok((count, total))
}
