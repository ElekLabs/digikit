use crate::mem::rd32;

/// Which consumer a call is about. Opaque to the service: it only compares ids and gives them back (`detach`, `describe`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ConsumerId(u8);

impl ConsumerId {
    pub const fn new(id: u8) -> ConsumerId {
        ConsumerId(id)
    }

    pub const fn get(self) -> u8 {
        self.0
    }
}

/// The records of one consumer for one service: `(kind, body)` in file order. A borrowed view for a host test; the runtime gives the `register` section of the
/// module file (`Consumer::of_section`).
pub type Records<'a> = &'a [(u32, &'a [u8])];

#[derive(Clone, Copy)]
enum Source<'a> {
    Slice(Records<'a>),
    /// A validated `register` section: 16 header bytes, then records of (kind, whole length, body).
    Section(*const u8, core::marker::PhantomData<&'a u8>),
}

/// A consumer that came up: its id, the address its image is loaded at and its records for the service being attached.
pub struct Consumer<'a> {
    id: ConsumerId,
    base: u32,
    source: Source<'a>,
}

impl<'a> Consumer<'a> {
    /// A consumer over a borrowed record list (a host test; its image base is 0).
    pub fn new(id: ConsumerId, records: Records<'a>) -> Consumer<'a> {
        Consumer {
            id,
            base: 0,
            source: Source::Slice(records),
        }
    }

    /// A consumer over the validated `register` section `reg` of the module record `id - 1`, loaded at `base`; a null `reg` is a module with no records.
    ///
    /// # Safety
    /// `reg` is null or a section that the registry walker has checked, readable for `'a`.
    pub unsafe fn of_section(id: ConsumerId, reg: *const u8, base: u32) -> Consumer<'a> {
        Consumer {
            id,
            base,
            source: Source::Section(reg, core::marker::PhantomData),
        }
    }

    pub fn id(&self) -> ConsumerId {
        self.id
    }

    /// The address the consumer's image is loaded at (callback addresses in its records are offsets from it).
    pub fn base(&self) -> u32 {
        self.base
    }

    /// The bodies of the records of `kind`, in file order.
    pub fn records(&self, kind: u32) -> RecordsOf<'a> {
        match self.source {
            Source::Slice(r) => RecordsOf::Slice(r.iter(), kind),
            Source::Section(reg, _) if reg.is_null() => RecordsOf::Slice([].iter(), kind),
            // SAFETY: `of_section` is given a checked section.
            Source::Section(reg, _) => RecordsOf::Section {
                reg,
                at: 16,
                left: unsafe { rd32(reg.add(12)) },
                kind,
                _from: core::marker::PhantomData,
            },
        }
    }
}

/// The iterator of [`Consumer::records`].
pub enum RecordsOf<'a> {
    Slice(core::slice::Iter<'a, (u32, &'a [u8])>, u32),
    Section {
        reg: *const u8,
        at: usize,
        left: u32,
        kind: u32,
        _from: core::marker::PhantomData<&'a u8>,
    },
}

impl<'a> Iterator for RecordsOf<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        match self {
            RecordsOf::Slice(it, kind) => it.find(|r| r.0 == *kind).map(|r| r.1),
            RecordsOf::Section {
                reg,
                at,
                left,
                kind,
                ..
            } => {
                while *left > 0 {
                    // SAFETY: `of_section` is given a checked section: each record's length is inside it.
                    let (k, len) = unsafe { (rd32(reg.add(*at)), rd32(reg.add(*at + 4)) as usize) };
                    let body = unsafe { reg.add(*at + 8) };
                    *at += len;
                    *left -= 1;
                    if k == *kind {
                        return Some(unsafe { core::slice::from_raw_parts(body, len - 8) });
                    }
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests;
