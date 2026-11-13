//! The write side of the `.syx` transport: ELE3 container build, HMAC seal, content checksum, 8-in-7
//! framing and per-packet checksums. A port of `tools/firmware/build.py` and `tools/firmware/authcode.py`.

use crate::aplib;
use crate::devdata::{self, DeviceInfo};
use crate::error::*;
use sha2::{Digest, Sha256};

pub const MFR: [u8; 3] = [0x00, 0x20, 0x3c];
pub const COUNT_OFF: usize = 0x1c;
pub const TABLE_OFF: usize = 0x20;
pub const ENTRY_SZ: usize = 16;
pub const FIRST_COUNTER: u32 = 242;
pub const CHUNK_SIZE: usize = 101;

/// A device the firmware container is for: one of `devdata::all()`.
#[derive(Clone, Copy, Debug)]
pub struct Device(&'static DeviceInfo);

impl PartialEq for Device {
    fn eq(&self, o: &Device) -> bool {
        self.0.short == o.0.short
    }
}
impl Eq for Device {}

/// The container seal: HMAC key material per device (`derive_key`). It is key material of the firmware format, not a device-profile
/// fact, and `devices/*.toml` are published as they are (the web page imports them), so it stays here, keyed by the device's short
/// name. A device file without an entry is refused when an image of it is read.
const SEALS: [(&str, &[u8], &str); 2] = [
    (
        "dt2",
        b"Master Overdrive",
        "695d82bca2f03f2c4457 0420eff1117831ea1aa03829c55e09065d1b790d63ff",
    ),
    (
        "dn2",
        b"Multiplier",
        "fc83032c7b2781c970dd306263156e8e07ff6fa0f312d2393cb2f5849f9670e5",
    ),
];

/// Whether `SEALS` holds a key for the device (checked against `[capabilities] seal` by `devdata`).
pub(crate) fn has_seal(short: &str) -> bool {
    SEALS.iter().any(|s| s.0 == short)
}

impl Device {
    pub fn from_id(id: u8) -> Result<Device> {
        let d = devdata::by_syx_id(id).ok_or_else(|| err!("unknown device id {id:#04x}"))?;
        if !has_seal(&d.short) {
            return Err(err!(
                "device {} has no container seal key (add it to SEALS in packages/patchkit/src/syx.rs)",
                d.short
            ));
        }
        Ok(Device(d))
    }
    pub fn short(self) -> &'static str {
        &self.0.short
    }
    /// The device byte of the SysEx framing.
    pub fn syx_id(self) -> u8 {
        self.0.syx_id
    }
    /// Limits of an image with no port: container, stored DSP, depacked DSP, staged MAIN (`[patch]` of the device file).
    pub fn limits(self) -> (usize, usize, usize, usize) {
        let d = self.0;
        (
            d.container_max as usize,
            d.dsp_stored_max as usize,
            d.dsp_depacked_max as usize,
            d.main_staged_max as usize,
        )
    }
    fn secret(self) -> (&'static [u8], [u8; 32]) {
        let s = SEALS
            .iter()
            .find(|s| s.0 == self.0.short)
            .expect("checked by from_id");
        (s.1, h32(s.2))
    }
}

fn h32(s: &str) -> [u8; 32] {
    let s: String = s.split_whitespace().collect();
    unhex(&s).unwrap().try_into().unwrap()
}

fn derive_key(device: Device) -> [u8; 32] {
    let (string, konst) = device.secret();
    let h1 = Sha256::digest(string);
    let rev: Vec<u8> = string.iter().rev().copied().collect();
    let h2 = Sha256::digest(&rev);
    let mut k = [0u8; 32];
    for i in 0..32 {
        k[i] = konst[i] ^ h1[i] ^ h2[i];
    }
    k
}

pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut block = [0u8; 64];
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut ipad = [0u8; 64];
    let mut opad = [0u8; 64];
    for i in 0..64 {
        ipad[i] = block[i] ^ 0x36;
        opad[i] = block[i] ^ 0x5c;
    }
    let mut h = Sha256::new();
    h.update(ipad);
    h.update(msg);
    let inner = h.finalize();
    let mut h = Sha256::new();
    h.update(opad);
    h.update(inner);
    h.finalize().into()
}

/// The 32-byte trailer for a container whose last 32 bytes are the (ignored) trailer slot.
pub fn trailer(container: &[u8], device: Device) -> [u8; 32] {
    hmac_sha256(&derive_key(device), &container[..container.len() - 32])
}

/// Container bytes ending in a 32-byte slot -> the same with the HMAC filled in.
pub fn seal(mut container: Vec<u8>, device: Device) -> Vec<u8> {
    let t = trailer(&container, device);
    let n = container.len();
    container[n - 32..].copy_from_slice(&t);
    container
}

pub fn content_checksum(container: &[u8]) -> u32 {
    let mut acc: u32 = 0;
    for (i, w) in container.as_chunks::<4>().0.iter().enumerate() {
        let word = u32::from_be_bytes([w[0], w[1], w[2], w[3]]);
        acc = acc.wrapping_add((i as u32 + 1) ^ word);
    }
    acc
}

pub fn packet_checksum(body: &[u8], k: u8) -> u8 {
    let mut total: u32 = k as u32;
    for i in 0..119 {
        total = total.wrapping_add((body[6 + i] ^ (i as u8).wrapping_add(k)) as u32);
    }
    (total & 0x7f) as u8
}

/// The 8-in-7 packing lives in `dkupload` (the module upload protocol uses the same one).
pub use dkupload::encode_8in7;

/// `[(id, dest, stored)]` -> container bytes with a zeroed 32-byte trailer slot.
///
/// `first_offset` is where the first payload starts (0x80 in `tools/firmware/build.py`; the stock images use 0x90, and
/// patchkit keeps the source image's value so that an unchanged image is rebuilt byte for byte).
pub fn build_container(
    header: &[u8],
    entries: &[(u32, u32, &[u8])],
    first_offset: usize,
) -> Result<Vec<u8>> {
    if header.len() != COUNT_OFF {
        return Err(err!("container header must be {COUNT_OFF} bytes"));
    }
    let n = entries.len();
    if TABLE_OFF + n * ENTRY_SZ > first_offset {
        return Err(err!(
            "too many sections for the {first_offset:#x} table area"
        ));
    }
    let mut out = vec![0u8; first_offset];
    out[..COUNT_OFF].copy_from_slice(header);
    out[COUNT_OFF..COUNT_OFF + 4].copy_from_slice(&(n as u32).to_be_bytes());
    let mut offset = first_offset;
    let mut body: Vec<u8> = Vec::new();
    for (i, (sid, dest, data)) in entries.iter().enumerate() {
        let e = TABLE_OFF + i * ENTRY_SZ;
        out[e..e + 4].copy_from_slice(&sid.to_be_bytes());
        out[e + 4..e + 8].copy_from_slice(&(offset as u32).to_be_bytes());
        out[e + 8..e + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
        out[e + 12..e + 16].copy_from_slice(&dest.to_be_bytes());
        body.extend_from_slice(data);
        let end = offset + data.len();
        if i != n - 1 {
            let pad = (16 - end % 16) % 16;
            body.extend(std::iter::repeat_n(0u8, pad));
            offset = end + pad;
        } else {
            offset = end;
        }
    }
    out.extend_from_slice(&body);
    let pad = (16 - out.len() % 16) % 16;
    out.extend(std::iter::repeat_n(0u8, pad));
    out.extend_from_slice(&[0u8; 32]);
    Ok(out)
}

fn patch_framing_count(msg: &[u8], n: usize) -> Vec<u8> {
    let mut m = msg.to_vec();
    // body = m[1..len-1]; count at body[11..14]
    m[1 + 11] = ((n >> 14) & 0x7f) as u8;
    m[1 + 12] = ((n >> 7) & 0x7f) as u8;
    m[1 + 13] = (n & 0x7f) as u8;
    m
}

/// Decoded stream -> `.syx` bytes, framing messages taken from the source image.
pub fn encode_syx(
    stream: &[u8],
    device_id: u8,
    framing_start: &[u8],
    framing_end: &[u8],
) -> Vec<u8> {
    let k = framing_start[1..framing_start.len() - 1][7];
    let n_msgs = stream.len().div_ceil(CHUNK_SIZE);
    let mut out = Vec::with_capacity(n_msgs * 128 + 32);
    out.extend_from_slice(&patch_framing_count(framing_start, n_msgs));
    for (counter, chunk) in (FIRST_COUNTER..).zip(stream.chunks(CHUNK_SIZE)) {
        let mut c = chunk.to_vec();
        c.resize(CHUNK_SIZE, 0);
        let payload = encode_8in7(&c);
        let mut body = Vec::with_capacity(126);
        body.extend_from_slice(&MFR);
        body.extend_from_slice(&[
            device_id,
            0x00,
            0x7e,
            ((counter >> 14) & 0x7f) as u8,
            ((counter >> 7) & 0x7f) as u8,
            (counter & 0x7f) as u8,
        ]);
        body.extend_from_slice(&payload);
        let ck = packet_checksum(&body, k) & 0x7f;
        body.push(ck);
        out.push(0xf0);
        out.extend_from_slice(&body);
        out.push(0xf7);
    }
    out.extend_from_slice(&patch_framing_count(framing_end, n_msgs));
    out
}

/// Split a `.syx` into its complete `F0 .. F7` messages.
pub fn messages(syx: &[u8]) -> Result<Vec<&[u8]>> {
    let mut v = Vec::new();
    let mut i = 0;
    while i < syx.len() {
        if syx[i] != 0xf0 {
            return Err(err!("expected F0 at offset {i}"));
        }
        let Some(rel) = syx[i..].iter().position(|&b| b == 0xf7) else {
            return Err(err!("unterminated SysEx message at {i}"));
        };
        v.push(&syx[i..=i + rel]);
        i += rel + 1;
    }
    Ok(v)
}

/// Section storage rules (tools/firmware/build.py `store_for_section`).
pub fn store_for_section(id: u32, data: &[u8]) -> Result<Vec<u8>> {
    match id {
        2 | 3 | 7 | 8 => Ok(aplib::pack_section(data)),
        4 => {
            let mut v = Vec::with_capacity(data.len() + 8);
            v.extend_from_slice(&(data.len() as u32).to_be_bytes());
            v.extend_from_slice(&0u32.to_be_bytes());
            v.extend_from_slice(data);
            Ok(v)
        }
        5 => Ok(data.to_vec()),
        _ => Err(err!("no known storage rule for section id {id}")),
    }
}
