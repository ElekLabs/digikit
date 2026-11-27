//! The module list section: its iterator and its replacement.
use super::*;
use dkrt_sdk::proto::{E_SET_FULL, E_SET_KEY};
use dkrt_sdk::settings::{get32, put16, put32, shift};

/// The modules of the module list, in order: (id, store hash).
pub struct Modules<'a> {
    b: &'a [u8],
    at: usize,
    end: usize,
}

pub fn modules(b: &[u8]) -> Modules<'_> {
    let used = get32(b, FMT.o_used()) as usize;
    let mut at = HEADER;
    while at < used {
        let Some(s) = sec_at(b, used, at) else { break };
        if s.tag == TAG_MODULES {
            return Modules {
                b,
                at: s.body,
                end: s.end,
            };
        }
        at = s.end;
    }
    Modules { b, at: 0, end: 0 }
}

impl<'a> Iterator for Modules<'a> {
    type Item = (&'a [u8], u32);
    fn next(&mut self) -> Option<Self::Item> {
        if self.at >= self.end {
            return None;
        }
        let n = self.b[self.at] as usize;
        if self.at + 1 + n + 4 > self.end {
            return None;
        }
        let id = &self.b[self.at + 1..self.at + 1 + n];
        let hash = get32(self.b, self.at + 1 + n);
        self.at += 1 + n + 4;
        Some((id, hash))
    }
}

/// Replace the module list. An empty list removes the section. `E_SET_FULL` when the image would pass `AREA` (nothing changes), `E_SET_KEY` for an id of 0 or over 255 bytes.
pub fn set_modules(b: &mut [u8], list: &[(&[u8], u32)]) -> Result<(), i32> {
    let used = get32(b, FMT.o_used()) as usize;
    let mut body = 0usize;
    for (id, _) in list {
        if id.is_empty() || id.len() > 255 {
            return Err(E_SET_KEY);
        }
        body += 1 + id.len() + 4;
    }
    let mut at = HEADER;
    let mut old: Option<Sec> = None;
    while at < used {
        let s = sec_at(b, used, at).ok_or(E_SET_KEY)?;
        if s.tag == TAG_MODULES {
            old = Some(s);
            break;
        }
        at = s.end;
    }
    let old_len = old.map(|s| s.end - s.at).unwrap_or(0);
    let new_len = if list.is_empty() { 0 } else { 3 + body };
    if body > 0xffff || used - old_len + new_len > AREA {
        return Err(E_SET_FULL);
    }
    // the module list goes first: remove the old one, make room at HEADER, write
    if old_len > 0 {
        let s = old.unwrap_or(Sec {
            at: 0,
            tag: 0,
            body: 0,
            end: 0,
        });
        shift(b, s.end, used, s.at);
    }
    let used = used - old_len;
    if new_len > 0 {
        shift(b, HEADER, used, HEADER + new_len);
        b[HEADER] = TAG_MODULES;
        put16(b, HEADER + 1, body as u32);
        let mut p = HEADER + 3;
        for (id, hash) in list {
            b[p] = id.len() as u8;
            p += 1;
            let mut i = 0;
            while i < id.len() {
                b[p + i] = id[i];
                i += 1;
            }
            p += id.len();
            put32(b, p, *hash);
            p += 4;
        }
    }
    put32(b, FMT.o_used(), (used + new_len) as u32);
    Ok(())
}
