//! The registration payload (version 1: text and file names; version 2: also integers, booleans and choices) and the declared
//! settings keys. A `Field` keeps offsets into the stored descriptor bytes, not the bounds of its kind.

use super::cursor::Cursor;
use super::kind::{extension, Kind, TEXT_MAX};
use dkrt_sdk::settings::{module_key, KEY_MAX};

pub(super) const DESCRIPTOR_MAX: usize = 512;
// Each field takes at least seven bytes. The version, display name and count take at least four.
pub(super) const FIELDS_MAX: usize = (DESCRIPTOR_MAX - 4) / 7;

#[derive(Clone, Copy)]
pub(super) struct Field {
    start: u16,
    len: u8,
    pub(super) max: u8,
    body: u16,
    kind: Kind,
}

impl Field {
    const NONE: Self = Self {
        start: 0,
        len: 0,
        max: 0,
        body: 0,
        kind: Kind::Text,
    };

    pub(super) fn key(self, descriptor: &[u8]) -> &[u8] {
        let start = self.start as usize;
        &descriptor[start..start + self.len as usize]
    }

    pub(super) fn kind(self) -> Kind {
        self.kind
    }

    fn body(self, descriptor: &[u8]) -> &[u8] {
        &descriptor[self.body as usize..]
    }

    /// The declared extension of a file-name field; empty for every other kind.
    pub(super) fn extension(self, descriptor: &[u8]) -> &[u8] {
        match self.kind {
            Kind::FileName => extension(self.body(descriptor)),
            _ => &[],
        }
    }

    /// Is `value` a stored value this field takes?
    pub(super) fn accepts(self, descriptor: &[u8], value: &[u8]) -> bool {
        self.kind.accepts(self.body(descriptor), self.max, value)
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
        if bytes.len() > DESCRIPTOR_MAX {
            return None;
        }
        let mut cursor = Cursor::new(bytes);
        let version = cursor.byte().filter(|v| matches!(v, 1 | 2))?;
        if cursor.text()?.is_empty() {
            return None;
        }
        let count = cursor.byte()? as usize;
        if count == 0 || count > FIELDS_MAX {
            return None;
        }
        let mut fields = [Field::NONE; FIELDS_MAX];
        let mut stored = [0; KEY_MAX];
        for index in 0..count {
            let start = (cursor.at() + 1) as u16;
            let key = cursor.string()?;
            module_key(module, key, &mut stored)?;
            if fields[..index].iter().any(|field| field.key(bytes) == key) {
                return None;
            }
            let label = cursor.text()?;
            let [max, tag, apply] = cursor.array()?;
            let kind = Kind::from_tag(tag).filter(|kind| kind.version() <= version)?;
            if label.is_empty() || max == 0 || max as usize > TEXT_MAX || apply > 1 {
                return None;
            }
            let body = cursor.at() as u16;
            kind.parse_body(&mut cursor, max)?;
            fields[index] = Field {
                start,
                len: key.len() as u8,
                max,
                body,
                kind,
            };
        }
        if !cursor.done() {
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

#[cfg(test)]
mod tests;
