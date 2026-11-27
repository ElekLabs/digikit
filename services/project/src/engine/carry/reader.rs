//! `CarryIn`: the reader of the bytes a reload carried. Every read answers `None` past the end.
pub(in crate::engine) struct CarryIn<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> CarryIn<'a> {
    pub(in crate::engine) fn new(buf: &'a [u8]) -> CarryIn<'a> {
        CarryIn { buf, at: 0 }
    }

    pub(in crate::engine) fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let b = self.buf.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(b)
    }

    pub(in crate::engine) fn u8(&mut self) -> Option<u8> {
        Some(self.bytes(1)?[0])
    }

    pub(in crate::engine) fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.bytes(2)?.try_into().ok()?))
    }

    pub(in crate::engine) fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.bytes(4)?.try_into().ok()?))
    }

    /// A flag and a value (see `CarryOut::opt_u32`).
    pub(in crate::engine) fn opt_u32(&mut self) -> Option<Option<u32>> {
        let flag = self.u8()?;
        let v = self.u32()?;
        match flag {
            0 => Some(None),
            1 => Some(Some(v)),
            _ => None,
        }
    }

    /// A deadline written by `CarryOut::deadline`, rebased on `now`; `None` for a time left of more than `DEADLINE_MAX_MS`.
    pub(in crate::engine) fn deadline(&mut self, now: u32) -> Option<u32> {
        let left = self.u32()?;
        (left <= super::DEADLINE_MAX_MS).then(|| now.wrapping_add(left))
    }

    pub(in crate::engine) fn flag(&mut self) -> Option<bool> {
        match self.u8()? {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }

    /// Whether every byte was read.
    pub(in crate::engine) fn is_done(&self) -> bool {
        self.at == self.buf.len()
    }
}
