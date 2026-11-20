//! Re-decode and check an output image: packet checksums, framing counts, content checksum, HMAC, section
//! streams, limits, the patch record, and (given the stock image) that the read-only sections are untouched.

use crate::dsp::Stream;
use crate::error::*;
use crate::firmware::Firmware;
use crate::module::Library;
use crate::port::Registry;
use crate::record::{self, Record};
use crate::syx;
use dt2_firmware_loader as loader;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Report {
    pub device: String,
    pub container_bytes: u64,
    /// The container limit of the image's port (the device default without a port). It applies to images for flashing only:
    /// `verify` does not check it, the flash checks do.
    pub container_max: u64,
    pub data_messages: u64,
    pub sections: Vec<(u32, u64, u64)>,
    pub record: Option<Record>,
    pub port: Option<String>,
    /// The bootstrap budget of the image (verify without `need_base` leaves it out).
    pub budget: Option<Budget>,
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

fn kb(n: i64) -> String {
    format!("{:.1} KB", n as f64 / 1024.0)
}

/// The in-place margin of the stored MAIN stream at the bootstrap's stage (`[patch] main_stage`), refused below `main_margin_min`, and the budget line:
/// the bootstrap unpacks MAIN over its own staged stream with no bounds check, so a stream whose output overtakes the unread input runs away before MAIN
/// starts (finding 152). Returns `bootstrap margin 9.1 KB, container 31.4 KB free`.
pub fn main_budget(fw: &Firmware, container_bytes: u64, container_max: u64) -> Result<Budget> {
    let main = fw
        .main()
        .ok_or_else(|| err!("the image has no MAIN OS section"))?;
    let (dest, stage, floor) = fw.device.main_unpack();
    let margin = crate::aplib::inplace_margin(&main.stored, dest, stage)
        .map_err(|e| err!("MAIN OS stream: {e}"))?;
    if let Some(m) = margin.filter(|m| *m < floor as i64) {
        return Err(err!(
            "the bootstrap would unpack MAIN OS over its own unread stream: in-place margin {m} bytes at the stage {stage:#x} (at least {floor} needed): the unit would hang before MAIN starts"
        ));
    }
    Ok(Budget {
        margin,
        stage,
        container_free: container_max as i64 - container_bytes as i64,
    })
}

/// The bootstrap budget of an image: the in-place margin of its MAIN stream at the device's stage (`None`: the unpack stays below the stage) and the
/// container space left under the limit. `Display` is the line `bootstrap margin 9.1 KB, container 31.4 KB free`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Budget {
    pub margin: Option<i64>,
    pub stage: u64,
    pub container_free: i64,
}

impl std::fmt::Display for Budget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.margin {
            Some(m) => write!(
                f,
                "bootstrap margin {}, container {} free",
                kb(m),
                kb(self.container_free)
            ),
            None => write!(
                f,
                "bootstrap margin: the unpack stays below the stage {:#x}, container {} free",
                self.stage,
                kb(self.container_free)
            ),
        }
    }
}

/// What the hook of a module that unpacks section `id` relies on, checked from the patch record's `repacks` entry for the section's role: the entry
/// must name a module of the record, the stream header length and byte sum must hold, the decoded section must be the stock one the entry names
/// (length and SHA-256), the stream must fit `stage_end - dest`, and the in-place margin must be above 0. With a library the module's own `[[repack]]`
/// must also say the same (a record that was edited, or a module that changed, is refused).
fn check_repacked(
    fw: &Firmware,
    id: u32,
    record: Option<&Record>,
    lib: Option<&Library>,
) -> Result<()> {
    let sec = fw.section(id).unwrap();
    let stored = &sec.stored;
    let dev = fw.device.short();
    let be = |b: &[u8]| u32::from_be_bytes(b.try_into().unwrap());
    if stored.len() < 8 || be(&stored[..4]) as usize + 8 != stored.len() {
        return Err(err!(
            "section {id}: the stored stream's header length does not match its size"
        ));
    }
    let sum = stored[8..]
        .iter()
        .fold(0u32, |s, &b| s.wrapping_add(b as u32));
    if sum != be(&stored[4..8]) {
        return Err(err!("section {id}: the stored stream's byte sum is wrong"));
    }
    let role = fw
        .device
        .repack_roles()
        .iter()
        .find(|r| fw.device.role_section(r) == Some(id))
        .ok_or_else(|| err!("section {id} cannot be stored packed on {dev}"))?;
    let entry = record
        .into_iter()
        .flat_map(|r| r.repacks.iter().map(move |e| (r, e)))
        .find(|(_, e)| e.role == *role);
    let Some((rec, e)) = entry else {
        return Err(err!(
            "section {id} is stored packed but the patch record has no repack entry for the {role} section: no module is known to unpack it"
        ));
    };
    if !rec.modules.contains(&e.module) {
        return Err(err!(
            "section {id}: the repack entry names {}, which is not a module of the patch record",
            e.module
        ));
    }
    if sec.data.len() as u64 != e.len as u64 || crate::error::sha256_hex(&sec.data) != e.sha256 {
        return Err(err!(
            "section {id} does not decode to the stock section that {} unpacks and checks",
            e.module
        ));
    }
    let len = stored.len() as u64;
    let (dest, stage_end) = (e.dest as u64, e.stage_end as u64);
    if stage_end < dest + len {
        return Err(err!(
            "section {id} ({len} bytes) does not fit the buffer {dest:#x}..{stage_end:#x} of {}",
            e.module
        ));
    }
    let margin = crate::aplib::inplace_margin(stored, dest, stage_end - len)
        .map_err(|x| err!("section {id}: {x}"))?;
    if let Some(m) = margin.filter(|m| *m <= 0) {
        return Err(err!(
            "section {id}: the in-place unpack of {} has margin {m}: it would overwrite bytes that are not read yet",
            e.module
        ));
    }
    if let Some(lib) = lib {
        let same = lib
            .get(e.module.split('@').next().unwrap_or(&e.module))
            .is_some_and(|m| {
                m.manifest.repack.iter().any(|r| {
                    r.role == e.role
                        && r.dest == dest
                        && r.stage_end == stage_end
                        && r.len == e.len as u64
                        && r.sha256 == e.sha256
                        && crate::prim::on(&r.devices, dev)
                })
            });
        if !same {
            return Err(err!(
                "section {id}: {} does not declare the repack the patch record names (dest, stage_end, length and hash must match)",
                e.module
            ));
        }
    }
    Ok(())
}

/// Verifies an image. A device with read-only sections (`[container] readonly`: bootstrap, FPGA or peer programs) needs `base`, the stock
/// image, to prove those sections unchanged; without it the call fails, so a damaged read-only section can never pass. The one exception
/// is an image whose SHA-256 is a known stock image (a port in `reg`): it is the stock image, so its read-only sections are stock.
pub fn verify(syx_bytes: &[u8], reg: Option<&Registry>, base: Option<&[u8]>) -> Result<Report> {
    check(syx_bytes, reg, base, true, None, true)
}

/// `verify` for an image made without a patch record (oracles, tests): a packed section has no repack entry to check, so only its decoded bytes are.
pub fn verify_unrecorded(
    syx_bytes: &[u8],
    reg: Option<&Registry>,
    base: Option<&[u8]>,
) -> Result<Report> {
    check(syx_bytes, reg, base, true, None, false)
}

/// `verify` with the module library: besides the patch record's repack entry (which `verify` checks alone), the claiming module's manifest must declare
/// the same layout and stock section.
pub fn verify_with_library(
    syx_bytes: &[u8],
    reg: Option<&Registry>,
    base: Option<&[u8]>,
    lib: &Library,
) -> Result<Report> {
    check(syx_bytes, reg, base, true, Some(lib), true)
}

/// Decodes and checks an image like `verify` but does not require the stock base: for identifying a file (what device, which record),
/// never as a proof that the image is safe to flash.
pub fn inspect(syx_bytes: &[u8], reg: Option<&Registry>) -> Result<Report> {
    check(syx_bytes, reg, None, false, None, true)
}

fn check(
    syx_bytes: &[u8],
    reg: Option<&Registry>,
    base: Option<&[u8]>,
    need_base: bool,
    lib: Option<&Library>,
    recorded: bool,
) -> Result<Report> {
    let mut checks = vec![];
    let mut budget: Option<Budget> = None;
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
    if let Some(main) = fw.main() {
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
    // The container limit is not checked here: it protects the flash path only (the flash checks read it from the report).
    let (container_max, stored_max, depacked_max, main_max) = match &limits {
        Some(l) => (
            l.container_max as usize,
            l.dsp_stored_max as usize,
            l.dsp_depacked_max as usize,
            l.main_staged_max as usize,
        ),
        None => fw.device.limits(),
    };
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
    if need_base {
        // `inspect` only identifies a file; a proof for flashing also holds the bootstrap's in-place unpack to its margin
        let b = main_budget(&fw, total as u64, container_max as u64)?;
        checks.push(b.to_string());
        budget = Some(b);
    }

    if base.is_none() && need_base && port.is_none() && !fw.device.readonly_sections().is_empty() {
        let ids: Vec<String> = fw
            .device
            .readonly_sections()
            .iter()
            .map(|i| i.to_string())
            .collect();
        return Err(err!(
            "the stock base image is required to verify a {} image: its read-only sections ({}) can only be checked against it \
             (this file's sha256 {} is not a known stock image)",
            fw.device.short(),
            ids.join(", "),
            fw.syx_sha256
        ));
    }
    if let Some(base_bytes) = base {
        let b = Firmware::parse(base_bytes)?;
        let mut repacked: Vec<u32> = vec![];
        for &id in fw.device.readonly_sections() {
            match (b.section(id), fw.section(id)) {
                (Some(x), Some(y)) if x.stored == y.stored && x.dest == y.dest => {}
                // a section that a module unpacks may be stored packed: what the image decodes to is still the stock bytes
                (Some(x), Some(y))
                    if fw.device.is_repackable(id) && x.data == y.data && x.dest == y.dest =>
                {
                    repacked.push(id)
                }
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
            .filter(|i| !repacked.contains(i))
            .map(|i| i.to_string())
            .collect();
        checks.push(format!(
            "read-only sections {} identical to the stock image",
            ids.join(", ")
        ));
        for id in repacked {
            if recorded {
                check_repacked(&fw, id, record.as_ref(), lib)?;
            }
            checks.push(format!(
                "section {id} is stored packed and decodes to the stock bytes{}",
                if !recorded {
                    " (the image has no patch record: not checked against one)"
                } else if lib.is_some() {
                    " (the record is consistent with it, and its module declares the same layout)"
                } else {
                    " (the record is consistent with it; MAIN's hook constants are not read)"
                }
            ));
        }
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
        container_max: container_max as u64,
        data_messages: data.len() as u64,
        sections: fw
            .sections
            .iter()
            .map(|s| (s.id, s.data.len() as u64, s.stored.len() as u64))
            .collect(),
        record,
        port: port_id,
        budget,
        checks,
    })
}
