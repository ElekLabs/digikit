//! Sample part of the DSP loader: the track-table record of a DSP section.
use crate::table::PoolCell;
use crate::tracks::Tracks;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::{in_sec, rd32le, DspSec, Job, FX_REC};
use dkrt_abi::mem::rd32;
use dkrt_abi::provider::Status;
use dkrt_abi::service::Claim;
use dkrt_sdk::*;

/// DSP section, version 5: the track-table record (`bank_off` is the record's offset in the section, validated here).
pub(crate) unsafe fn check_sections(sec: &DspSec) -> Result<(), Status> {
    if sec.version != 5 {
        return Ok(());
    }
    let DspSec {
        s,
        size,
        image_off,
        image_size,
        reloc_off,
        nreloc,
        import_off,
        nimport,
        voice_off,
        nvoice,
        vstride,
        fx_off,
        nfx,
        bank_off,
        ..
    } = *sec;
    if bank_off < 64 || bank_off & 3 != 0 || !in_sec(bank_off, 32, size) {
        return Err(E_DSP_FORMAT);
    }
    // The record must not alias another section table or the initialized image.
    for (o, n) in [
        (image_off, image_size),
        (reloc_off, nreloc * 8),
        (import_off, nimport * 32),
        (voice_off, nvoice * vstride),
        (fx_off, nfx * FX_REC),
    ] {
        if n != 0 && bank_off < o + n && o < bank_off + 32 {
            return Err(E_DSP_FORMAT);
        }
    }
    let regions = [
        (image_off, image_size),
        (reloc_off, nreloc * 8),
        (import_off, nimport * 32),
        (voice_off, nvoice * vstride),
        (fx_off, nfx * FX_REC),
    ];
    for (i, &(o, n)) in regions.iter().enumerate() {
        if n != 0 && o < 64 {
            return Err(E_DSP_FORMAT);
        }
        for &(p, m) in &regions[..i] {
            if n != 0 && m != 0 && o < p + m && p < o + n {
                return Err(E_DSP_FORMAT);
            }
        }
    }
    let b = s.add(bank_off as usize);
    let record = core::slice::from_raw_parts(b, 32);
    let image = core::slice::from_raw_parts(s.add(image_off as usize), image_size as usize);
    // The record is a per-track table (kind 2) with its two blocks.
    if rd32le(b) != 2 || !dkrt_sdk::sample_job::valid_tracks(record, image) {
        return Err(E_DSP_FORMAT);
    }
    // Relocations cannot turn the initially empty table into a published one.
    let (records, control) = (rd32le(b.add(8)), rd32le(b.add(12)));
    for k in 0..nreloc {
        let at = rd32(s.add((reloc_off + k * 8) as usize));
        if (at < records + dkrt_sdk::sample_job::TRACK_RECORDS_BYTES
            && records < at.saturating_add(6))
            || (at < control + dkrt_sdk::sample_job::TRACK_CONTROL_BYTES
                && control < at.saturating_add(6))
        {
            return Err(E_DSP_FORMAT);
        }
    }
    Ok(())
}

/// A track table needs a free client slot: refuse before anything is reserved.
pub(crate) unsafe fn admit(tracks: &Tracks, job: &Job) -> Result<(), Status> {
    if !job.bank.is_null() && rd32le(job.bank) == 2 && !tracks.can_reserve() {
        return Err(E_TABLE_FULL);
    }
    Ok(())
}

/// Bind the module's track table to a client slot (`admit` checked one free).
pub(crate) unsafe fn claim<C: CoreApi>(
    core: &C,
    table: &PoolCell,
    tracks: &mut Tracks,
    c: &mut Claim,
) {
    let (rec, job, base) = (c.rec, c.job, c.base);
    if !job.bank.is_null() {
        tracks.reserve(
            core,
            table,
            rec,
            rd32le(job.bank.add(4)),
            base + rd32le(job.bank.add(8)),
            base + rd32le(job.bank.add(12)),
        );
    }
}
