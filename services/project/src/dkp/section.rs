//! Sections of the file: where one starts, its owner, the section of an owner.
use super::*;
use dkrt_sdk::settings::get16;

/// A section: where it starts, its tag and where its body lies.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Sec {
    pub(super) at: usize,
    pub(super) tag: u8,
    pub(super) body: usize,
    pub(super) end: usize,
}

pub(super) fn sec_at(b: &[u8], used: usize, at: usize) -> Option<Sec> {
    if at + 3 > used || used > b.len() {
        return None;
    }
    let len = get16(b, at + 1) as usize;
    if at + 3 + len > used {
        return None;
    }
    Some(Sec {
        at,
        tag: b[at],
        body: at + 3,
        end: at + 3 + len,
    })
}

pub(super) fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

pub(super) fn owner_of(b: &[u8], s: &Sec) -> Option<(usize, usize)> {
    if s.tag != TAG_DATA || s.body >= s.end {
        return None;
    }
    let n = b[s.body] as usize;
    if n == 0 || s.body + 1 + n > s.end {
        return None;
    }
    Some((s.body + 1, n))
}

pub(super) fn find_section(b: &[u8], used: usize, owner: &[u8]) -> Option<Sec> {
    let mut at = HEADER;
    while at < used {
        let s = sec_at(b, used, at)?;
        if let Some((o, n)) = owner_of(b, &s) {
            if same(&b[o..o + n], owner) {
                return Some(s);
            }
        }
        at = s.end;
    }
    None
}

/// Does the key start with `<owner>/`?
pub(super) fn has_prefix(key: &[u8], owner: &[u8]) -> bool {
    key.len() > owner.len() + 1 && same(&key[..owner.len()], owner) && key[owner.len()] == b'/'
}
