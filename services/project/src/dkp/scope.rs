//! The three calls of the project scope: `get`, `put` and `remove` on the image.
use super::*;
use dkrt_sdk::proto::{E_SET_FULL, E_SET_KEY, E_SET_NOKEY, E_SET_VALUE};
use dkrt_sdk::settings::{
    find_from, get16, get32, module_key, put16, put32, shift, KEY_MAX, VALUE_MAX,
};

/// `dk_settings_get` on the project scope: the value of `<owner>/<bare>` into `out`; its length, `E_SET_NOKEY`, `E_SET_KEY` (a key the rules refuse) or `E_SET_VALUE` (`out` too small).
pub fn get(b: &[u8], owner: &[u8], bare: &[u8], out: &mut [u8]) -> i32 {
    let mut full = [0u8; KEY_MAX];
    let Some(kn) = module_key(owner, bare, &mut full) else {
        return E_SET_KEY;
    };
    let used = get32(b, FMT.o_used()) as usize;
    let Some(s) = find_section(b, used, owner) else {
        return E_SET_NOKEY;
    };
    let Some((o, n)) = owner_of(b, &s) else {
        return E_SET_NOKEY;
    };
    match find_from(b, o + n, s.end, &full[..kn]) {
        None => E_SET_NOKEY,
        Some(e) => {
            if e.value.1 > out.len() {
                return E_SET_VALUE;
            }
            let mut i = 0;
            while i < e.value.1 {
                out[i] = b[e.value.0 + i];
                i += 1;
            }
            e.value.1 as i32
        }
    }
}

/// `dk_settings_put` on the project scope: set `<owner>/<bare>`. `Ok(true)` when the image changed, `Ok(false)` when the key already had this value, `E_SET_KEY`, `E_SET_VALUE`
/// (over 1024 bytes) or `E_SET_FULL` (the image would pass 16 KiB: it is left as it was).
pub fn put(b: &mut [u8], owner: &[u8], bare: &[u8], value: &[u8]) -> Result<bool, i32> {
    let mut full = [0u8; KEY_MAX];
    let kn = module_key(owner, bare, &mut full).ok_or(E_SET_KEY)?;
    let key = &full[..kn];
    if value.len() > VALUE_MAX {
        return Err(E_SET_VALUE);
    }
    let used = get32(b, FMT.o_used()) as usize;
    let entry_len = 1 + kn + 2 + value.len();
    match find_section(b, used, owner) {
        None => {
            let body = 1 + owner.len() + entry_len;
            if used + 3 + body > AREA {
                return Err(E_SET_FULL);
            }
            let mut p = used;
            b[p] = TAG_DATA;
            put16(b, p + 1, body as u32);
            b[p + 3] = owner.len() as u8;
            p += 4;
            let mut i = 0;
            while i < owner.len() {
                b[p + i] = owner[i];
                i += 1;
            }
            p += owner.len();
            write_entry(b, p, key, value);
            put32(b, FMT.o_used(), (used + 3 + body) as u32);
            Ok(true)
        }
        Some(s) => {
            let (o, n) = owner_of(b, &s).ok_or(E_SET_KEY)?;
            match find_from(b, o + n, s.end, key) {
                Some(e) => {
                    let old = e.value.1;
                    if same(&b[e.value.0..e.value.0 + old], value) {
                        return Ok(false);
                    }
                    let new_used = used - old + value.len();
                    if new_used > AREA {
                        return Err(E_SET_FULL);
                    }
                    shift(b, e.end, used, e.value.0 + value.len());
                    put16(b, e.value.0 - 2, value.len() as u32);
                    let mut i = 0;
                    while i < value.len() {
                        b[e.value.0 + i] = value[i];
                        i += 1;
                    }
                    let len = get16(b, s.at + 1) as usize - old + value.len();
                    put16(b, s.at + 1, len as u32);
                    put32(b, FMT.o_used(), new_used as u32);
                    Ok(true)
                }
                None => {
                    if used + entry_len > AREA {
                        return Err(E_SET_FULL);
                    }
                    shift(b, s.end, used, s.end + entry_len);
                    write_entry(b, s.end, key, value);
                    let len = get16(b, s.at + 1) as usize + entry_len;
                    put16(b, s.at + 1, len as u32);
                    put32(b, FMT.o_used(), (used + entry_len) as u32);
                    Ok(true)
                }
            }
        }
    }
}

pub(super) fn write_entry(b: &mut [u8], at: usize, key: &[u8], value: &[u8]) {
    b[at] = key.len() as u8;
    let mut p = at + 1;
    let mut i = 0;
    while i < key.len() {
        b[p + i] = key[i];
        i += 1;
    }
    p += key.len();
    put16(b, p, value.len() as u32);
    p += 2;
    i = 0;
    while i < value.len() {
        b[p + i] = value[i];
        i += 1;
    }
}

/// `dk_settings_remove` on the project scope: `Ok(true)` when the key was there, `Ok(false)` when not, `E_SET_KEY`. The last entry of an owner takes its section with it.
pub fn remove(b: &mut [u8], owner: &[u8], bare: &[u8]) -> Result<bool, i32> {
    let mut full = [0u8; KEY_MAX];
    let kn = module_key(owner, bare, &mut full).ok_or(E_SET_KEY)?;
    let used = get32(b, FMT.o_used()) as usize;
    let Some(s) = find_section(b, used, owner) else {
        return Ok(false);
    };
    let Some((o, n)) = owner_of(b, &s) else {
        return Ok(false);
    };
    let Some(e) = find_from(b, o + n, s.end, &full[..kn]) else {
        return Ok(false);
    };
    let cut = e.end - e.at;
    let len = get16(b, s.at + 1) as usize - cut;
    if len == 1 + n {
        // the section's only entry: the section goes
        let sec_len = s.end - s.at;
        shift(b, s.end, used, s.at);
        put32(b, FMT.o_used(), (used - sec_len) as u32);
        return Ok(true);
    }
    shift(b, e.end, used, e.at);
    put16(b, s.at + 1, len as u32);
    put32(b, FMT.o_used(), (used - cut) as u32);
    Ok(true)
}
