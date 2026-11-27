//! A read position in a descriptor: length-prefixed strings and fixed-size words, each `None` when the bytes end early.

pub(super) struct Cursor<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// The offset of the next byte.
    pub(super) fn at(&self) -> usize {
        self.at
    }

    pub(super) fn done(&self) -> bool {
        self.at == self.bytes.len()
    }

    pub(super) fn byte(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }

    pub(super) fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let word = self.bytes.get(self.at..self.at + N)?;
        self.at += N;
        word.try_into().ok()
    }

    /// A string of one length byte and that many bytes.
    pub(super) fn string(&mut self) -> Option<&'a [u8]> {
        let size = self.byte()? as usize;
        let text = self.bytes.get(self.at..self.at + size)?;
        self.at += size;
        Some(text)
    }

    pub(super) fn text(&mut self) -> Option<&'a str> {
        core::str::from_utf8(self.string()?).ok()
    }
}
