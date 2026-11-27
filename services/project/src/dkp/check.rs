//! The checks a file passes before its image is used.
use super::*;
use dkrt_sdk::settings::{entry_at, get16, get32};

/// Check a file as read from the store: the codec (magic, version, bounds, crc32), the device, the header's own bounds and every section. `device` is this device's id.
pub fn check(b: &[u8], device: u16) -> Result<Head, Ignore> {
    if b.len() < HEADER || dkrt_sdk::settings::get32(b, 0) != MAGIC {
        return Err(Ignore::Format);
    }
    if get16(b, 4) != VERSION as u32 {
        return Err(Ignore::Version);
    }
    let (_, used) = FMT.peek(b).ok_or(Ignore::Format)?;
    if !FMT.crc_ok(b, used) {
        return Err(Ignore::Format);
    }
    let h = head_of(b);
    if h.device != device {
        return Err(Ignore::Device);
    }
    if !head_ok(&h) {
        return Err(Ignore::Format);
    }
    sections_ok(b, used)?;
    Ok(h)
}

/// The header's own bounds: the slot and the name length.
pub fn head_ok(h: &Head) -> bool {
    h.slot <= 127 && h.name_len <= 16
}

/// Is `b` (the live image, which is not sealed) an image of the codec whose sections pass the checks of a file? The crc32 and the device are not checked: a put does not keep the
/// crc32 and the carried head has been checked on its own.
pub fn image_ok(b: &[u8]) -> bool {
    if b.len() < HEADER || get32(b, 0) != MAGIC || get16(b, 4) != VERSION as u32 {
        return false;
    }
    let Some((_, used)) = FMT.peek(b) else {
        return false;
    };
    head_ok(&head_of(b)) && sections_ok(b, used).is_ok()
}

pub(super) fn sections_ok(b: &[u8], used: usize) -> Result<(), Ignore> {
    let mut at = HEADER;
    let mut modules = false;
    while at < used {
        let s = sec_at(b, used, at).ok_or(Ignore::Format)?;
        if s.tag == TAG_MODULES {
            if modules {
                return Err(Ignore::Format);
            }
            modules = true;
            let mut p = s.body;
            while p < s.end {
                let n = b[p] as usize;
                if n == 0 || p + 1 + n + 4 > s.end {
                    return Err(Ignore::Format);
                }
                p += 1 + n + 4;
            }
        } else if s.tag == TAG_DATA {
            let (o, n) = owner_of(b, &s).ok_or(Ignore::Format)?;
            // one section of an owner
            if let Some(f) = find_section(b, used, &b[o..o + n]) {
                if f.at != s.at {
                    return Err(Ignore::Format);
                }
            }
            let mut p = o + n;
            while p < s.end {
                let e = entry_at(b, s.end, p).ok_or(Ignore::Format)?;
                let key = &b[e.key.0..e.key.0 + e.key.1];
                if !has_prefix(key, &b[o..o + n]) {
                    return Err(Ignore::Format);
                }
                // a key twice: sections are small
                let mut q = o + n;
                while q < p {
                    let f = entry_at(b, s.end, q).ok_or(Ignore::Format)?;
                    if same(&b[f.key.0..f.key.0 + f.key.1], key) {
                        return Err(Ignore::Format);
                    }
                    q = f.end;
                }
                p = e.end;
            }
        }
        at = s.end;
    }
    Ok(())
}

/// Does the image hold digikit data: a module list with a module, or a section of an owner? (A file is written only then.)
pub fn has_data(b: &[u8]) -> bool {
    let used = get32(b, FMT.o_used()) as usize;
    let mut at = HEADER;
    while at < used {
        let Some(s) = sec_at(b, used, at) else {
            return false;
        };
        if s.tag == TAG_DATA || (s.tag == TAG_MODULES && s.end > s.body) {
            return true;
        }
        at = s.end;
    }
    false
}
