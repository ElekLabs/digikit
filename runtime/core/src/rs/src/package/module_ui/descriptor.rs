//! Version 1 registration payload and the declared settings keys.

use dkrt_sdk::settings::{module_key, KEY_MAX};

pub(super) const DESCRIPTOR_MAX: usize = 512;
pub(super) const TEXT_MAX: usize = 64;
const FILE_NAME_MAX: usize = 47;
pub(super) const EXTENSION_MAX: usize = 16;
// Each field takes at least seven bytes. The version, display name and count take at least four.
pub(super) const FIELDS_MAX: usize = (DESCRIPTOR_MAX - 4) / 7;

#[derive(Clone, Copy)]
pub(super) struct Field {
    start: u16,
    len: u8,
    pub(super) max: u8,
    extension_start: u16,
    extension_len: u8,
}

impl Field {
    const NONE: Self = Self {
        start: 0,
        len: 0,
        max: 0,
        extension_start: 0,
        extension_len: 0,
    };

    pub(super) fn key(self, descriptor: &[u8]) -> &[u8] {
        let start = self.start as usize;
        &descriptor[start..start + self.len as usize]
    }

    pub(super) fn extension(self, descriptor: &[u8]) -> &[u8] {
        let start = self.extension_start as usize;
        &descriptor[start..start + self.extension_len as usize]
    }
}

pub(super) struct Descriptor {
    bytes: [u8; DESCRIPTOR_MAX],
    len: u16,
    fields: [Field; FIELDS_MAX],
    count: u8,
}

impl Descriptor {
    pub(super) const EMPTY: Self = Self {
        bytes: [0; DESCRIPTOR_MAX],
        len: 0,
        fields: [Field::NONE; FIELDS_MAX],
        count: 0,
    };

    pub(super) fn parse(module: &[u8], bytes: &[u8]) -> Option<Self> {
        if bytes.len() > DESCRIPTOR_MAX || bytes.first().copied() != Some(1) {
            return None;
        }
        let mut at = 1;
        let name = take_text(bytes, &mut at)?;
        if name.is_empty() {
            return None;
        }
        let count = *bytes.get(at)? as usize;
        at += 1;
        if count == 0 || count > FIELDS_MAX {
            return None;
        }
        let mut fields = [Field::NONE; FIELDS_MAX];
        let mut stored = [0; KEY_MAX];
        for index in 0..count {
            let start = (at + 1) as u16;
            let key = take_bytes(bytes, &mut at)?;
            module_key(module, key, &mut stored)?;
            if fields[..index].iter().any(|field| field.key(bytes) == key) {
                return None;
            }
            let label = take_text(bytes, &mut at)?;
            let &[max, kind, apply] = bytes.get(at..at + 3)? else {
                return None;
            };
            at += 3;
            if label.is_empty() || max == 0 || max as usize > TEXT_MAX || apply > 1 {
                return None;
            }
            let mut field = Field {
                start,
                len: key.len() as u8,
                max,
                ..Field::NONE
            };
            match kind {
                0 => (),
                1 => {
                    field.extension_start = (at + 1) as u16;
                    let ext = take_bytes(bytes, &mut at)?;
                    if !(1..=EXTENSION_MAX).contains(&ext.len())
                        || !(ext.len() + 2..=FILE_NAME_MAX).contains(&(max as usize))
                        || !ext
                            .iter()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                    {
                        return None;
                    }
                    field.extension_len = ext.len() as u8;
                    if take_text(bytes, &mut at)?.is_empty() {
                        return None;
                    }
                }
                _ => return None,
            }
            fields[index] = field;
        }
        if at != bytes.len() {
            return None;
        }
        let mut raw = [0; DESCRIPTOR_MAX];
        raw[..bytes.len()].copy_from_slice(bytes);
        Some(Self {
            bytes: raw,
            len: bytes.len() as u16,
            fields,
            count: count as u8,
        })
    }

    pub(super) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    pub(super) fn count(&self) -> usize {
        self.count as usize
    }

    pub(super) fn field(&self, index: u8) -> Option<Field> {
        self.fields[..self.count()].get(index as usize).copied()
    }

    pub(super) fn key_index(&self, key: &[u8]) -> Option<usize> {
        self.fields[..self.count()]
            .iter()
            .position(|field| field.key(self.bytes()) == key)
    }
}

fn take_bytes<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a [u8]> {
    let size = *bytes.get(*at)? as usize;
    *at += 1;
    let text = bytes.get(*at..*at + size)?;
    *at += size;
    Some(text)
}

fn take_text<'a>(bytes: &'a [u8], at: &mut usize) -> Option<&'a str> {
    core::str::from_utf8(take_bytes(bytes, at)?).ok()
}

/// A store leaf usable by `dk_data_path`, with a nonempty stem and the declared extension.
pub(super) fn file_name_ok(leaf: &[u8], extension: &[u8]) -> bool {
    let Some(dot) = leaf.iter().rposition(|&c| c == b'.') else {
        return false;
    };
    dot != 0
        && leaf[dot + 1..].eq_ignore_ascii_case(extension)
        && !leaf.windows(2).any(|pair| pair == b"..")
        && leaf
            .iter()
            .all(|c| (0x20..=0x7e).contains(c) && !matches!(c, b'/' | b'\\'))
}
