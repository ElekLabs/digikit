//! Re-decode and check an output image: packet checksums, framing counts, content checksum, HMAC, section
//! streams, limits, the patch record, and (given the stock image) that the read-only sections are untouched.

use crate::dsp::Stream;
use crate::error::*;
use crate::firmware::Firmware;
use crate::port::Registry;
use crate::record::{self, Record};
use crate::syx;
use dt2_firmware_loader as loader;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub device: String,
    pub container_bytes: u64,
    pub data_messages: u64,
    pub sections: Vec<(u32, u64, u64)>,
    pub record: Option<Record>,
    pub port: Option<String>,
    pub checks: Vec<String>,
}

/// The segment chain behind the loader the reset hook calls: `(dest, payload, bss)` each, empty when the
/// MAIN OS is not hooked. The chain must end exactly at the end of the image.
pub fn read_segments(main: &[u8], base: u64) -> Result<Vec<(u64, Vec<u8>, u64)>> {
    const HOOK: u64 = 0x4000_053e;
    let at = |a: u64, n: usize| -> Result<&[u8]> {
        let o = a.checked_sub(base).map(|o| o as usize);
        match o {
            Some(o) if o + n <= main.len() => Ok(&main[o..o + n]),
            _ => Err(err!("MAIN OS address {a:#x} is outside the image")),
        }
    };
    let be = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap()) as u64;
    let h = at(HOOK, 8)?;
    if h[..2] != [0x4e, 0xb9] || h[6..] != [0x4e, 0x71] {
        return Ok(vec![]);
    }
    let loader = be(&h[2..6]);
    let l = at(loader, 6)?;
    if l[..2] != [0x41, 0xf9] {
        return Err(err!(
            "the reset hook calls {loader:#x}, which is not a patchkit loader"
        ));
    }
    let mut p = be(&l[2..6]);
    let mut out = vec![];
    loop {
        let len = be(at(p, 4)?);
        p += 4;
        if len == u32::MAX as u64 {
            break;
        }
        let dest = be(at(p, 4)?);
        let bss = be(at(p + 4, 4)?);
        p += 8;
        let payload = at(p, len as usize)?.to_vec();
        p += len;
        out.push((dest, payload, bss));
    }
    if p != base + main.len() as u64 {
        return Err(err!(
            "the segment chain ends at {p:#x}, not at the image end {:#x}",
            base + main.len() as u64
        ));
    }
    Ok(out)
}

/// The `DKPR` record of an image: the first expansion segment of MAIN, or a non-fill block of the DSP
/// stream (which wins when both exist).
fn find_record(fw: &Firmware) -> Result<Option<Record>> {
    let mut record = None;
    if let Some(main) = fw.main() {
        let segs = read_segments(&main.data, main.dest as u64)?;
        if let Some((_, payload, _)) = segs.first()
            && payload.starts_with(record::MAGIC)
        {
            record = Some(Record::decode(payload)?);
        }
    }
    if let Some(dsp) = fw.dsp() {
        let stream = Stream::parse(dsp.data.clone())?;
        for b in &stream.blocks {
            if !b.fill()
                && b.count >= 4
                && &stream.blob[b.payload_offset..b.payload_offset + 4] == record::MAGIC
            {
                record = Some(Record::decode(
                    &stream.blob[b.payload_offset..b.payload_offset + b.count as usize],
                )?);
            }
        }
    }
    Ok(record)
}

/// The `DKPR` patch record of a patched image (its stock base and modules), or `None` for an image
/// patchkit did not make. Reads the record only; it does not verify the image.
pub fn patch_record(syx_bytes: &[u8]) -> Result<Option<Record>> {
    find_record(&Firmware::parse(syx_bytes)?)
}

pub fn verify(syx_bytes: &[u8], reg: Option<&Registry>, base: Option<&[u8]>) -> Result<Report> {
    let mut checks = vec![];
    let msgs = syx::messages(syx_bytes)?;
    if msgs.len() < 3 || msgs[0].len() != 16 || msgs[msgs.len() - 1].len() != 16 {
        return Err(err!("missing framing messages"));
    }
    let k = msgs[0][1..15][7];
    let data: Vec<&[u8]> = msgs[1..msgs.len() - 1].to_vec();
    let count =
        |m: &[u8]| (m[1 + 11] as usize) * 16384 + (m[1 + 12] as usize) * 128 + m[1 + 13] as usize;
    if count(msgs[0]) != data.len() || count(msgs[msgs.len() - 1]) != data.len() {
        return Err(err!(
            "framing message count does not match the {} data messages",
            data.len()
        ));
    }
    for (i, m) in data.iter().enumerate() {
        if m.len() != 128 {
            return Err(err!("data message {i} has {} bytes", m.len()));
        }
        let body = &m[1..m.len() - 1];
        if body[125] != syx::packet_checksum(body, k) {
            return Err(err!("data message {i}: bad packet checksum"));
        }
        let c = 242 + i;
        if body[6] as usize != (c >> 14) & 0x7f
            || body[7] as usize != (c >> 7) & 0x7f
            || body[8] as usize != c & 0x7f
        {
            return Err(err!("data message {i}: bad counter"));
        }
    }
    checks.push(format!(
        "{} data messages: counters, packet checksums, framing count",
        data.len()
    ));

    let fw = Firmware::parse(syx_bytes)?;
    let dec = loader::decode_syx(syx_bytes)?;
    let total = u32::from_be_bytes(dec[0..4].try_into().unwrap()) as usize;
    let stored_sum = u32::from_be_bytes(dec[4..8].try_into().unwrap());
    let container = &dec[8..8 + total];
    if !total.is_multiple_of(16) || syx::content_checksum(container) != stored_sum {
        return Err(err!("content checksum does not match the container"));
    }
    if !syx::trailer_matches(container, fw.device) {
        return Err(err!("HMAC trailer does not match"));
    }
    checks.push(if fw.device.sealed() {
        "content checksum and HMAC trailer".into()
    } else {
        "content checksum (the device has no seal)".into()
    });

    // Sections decoded by the firmware's own rules (the loader checks stream length, byte sum, end marker).
    let parsed = loader::parse(syx_bytes)?;
    for s in &parsed.sections {
        if let loader::SectionEncoding::Elz { .. } = s.encoding {
            // depack succeeded
        }
    }
    checks.push(format!("{} sections decode", parsed.sections.len()));

    let port = reg.and_then(|r| r.find(&fw.syx_sha256).or(None));
    // a device without a DSP has no boot stream to parse and no stored/depacked limits to check
    let dsp = match fw.device.dsp_section() {
        Some(_) => {
            let dsp = fw.dsp().ok_or_else(|| err!("no DSP section"))?;
            let stream = Stream::parse(dsp.data.clone())?;
            crate::bootrom::require(&stream.blob)?;
            Some(dsp)
        }
        None => None,
    };
    let record = find_record(&fw)?;
    let mut segments: Vec<(u64, u64, u64)> = vec![];
    if let Some(main) = fw.section(3) {
        let segs = read_segments(&main.data, main.dest as u64)?;
        for (dest, payload, bss) in &segs {
            segments.push((*dest, payload.len() as u64, *bss));
        }
        if !segs.is_empty() {
            checks.push(format!(
                "{} expansion segments, chain ends at the image end",
                segs.len()
            ));
        }
    }
    let mut port_id = None;
    let in_dsp = record.is_none();
    let _ = in_dsp;
    let limits = if let Some(rec) = &record {
        let p = reg.and_then(|r| {
            r.ports
                .iter()
                .find(|p| p.port.syx_sha256 == rec.base_sha256)
        });
        port_id = p.map(|p| p.id());
        p.map(|p| p.limits.clone())
    } else {
        port.map(|p| {
            port_id = Some(p.id());
            p.limits.clone()
        })
    };
    let (container_max, stored_max, depacked_max, main_max) = match &limits {
        Some(l) => (
            l.container_max as usize,
            l.dsp_stored_max as usize,
            l.dsp_depacked_max as usize,
            l.main_staged_max as usize,
        ),
        None => fw.device.limits(),
    };
    if total > container_max {
        return Err(err!(
            "container is {total} bytes, over the {container_max} limit"
        ));
    }
    if let Some(dsp) = dsp
        && (dsp.stored.len() > stored_max || dsp.data.len() > depacked_max)
    {
        return Err(err!(
            "DSP section stored {} / depacked {} exceeds {stored_max} / {depacked_max}",
            dsp.stored.len(),
            dsp.data.len()
        ));
    }
    if let Some(m) = fw.main()
        && m.data.len() > main_max
    {
        return Err(err!(
            "MAIN OS is {} bytes, over the {main_max} staging limit",
            m.data.len()
        ));
    }
    checks.push("size limits".into());

    if let Some(base_bytes) = base {
        let b = Firmware::parse(base_bytes)?;
        for &id in fw.device.readonly_sections() {
            match (b.section(id), fw.section(id)) {
                (Some(x), Some(y)) if x.stored == y.stored && x.dest == y.dest => {}
                (None, None) => {}
                _ => {
                    return Err(err!(
                        "section {id} differs from the stock image (never allowed)"
                    ));
                }
            }
        }
        if let Some(rec) = &record
            && rec.base_sha256 != b.syx_sha256
        {
            return Err(err!(
                "the record names base {} but the stock image is {}",
                rec.base_sha256,
                b.syx_sha256
            ));
        }
        let ids: Vec<String> = fw
            .device
            .readonly_sections()
            .iter()
            .map(|i| i.to_string())
            .collect();
        checks.push(format!(
            "read-only sections {} identical to the stock image",
            ids.join(", ")
        ));
    }
    if let Some(pid) = &port_id
        && let Some(p) = reg.and_then(|r| r.by_id(pid))
        && let Some(r) = p.region("cf.expansion")
    {
        for (dest, len, bss) in &segments {
            if *dest < r.start || dest + len + bss > r.end {
                return Err(err!(
                    "segment {dest:#x}+{len}+{bss} is outside cf.expansion {:#x}..{:#x}",
                    r.start,
                    r.end
                ));
            }
        }
    }
    Ok(Report {
        device: fw.device.short().into(),
        container_bytes: total as u64,
        data_messages: data.len() as u64,
        sections: fw
            .sections
            .iter()
            .map(|s| (s.id, s.data.len() as u64, s.stored.len() as u64))
            .collect(),
        record,
        port: port_id,
        checks,
    })
}
