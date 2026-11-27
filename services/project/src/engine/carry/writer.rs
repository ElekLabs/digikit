//! `CarryOut`: the writer of the bytes a reload carries. A write past the end is remembered, not made: `finish` answers 0 bytes then.
pub(in crate::engine) struct CarryOut<'a> {
    buf: &'a mut [u8],
    at: usize,
    fits: bool,
}

impl<'a> CarryOut<'a> {
    pub(in crate::engine) fn new(buf: &'a mut [u8]) -> CarryOut<'a> {
        CarryOut {
            buf,
            at: 0,
            fits: true,
        }
    }

    pub(in crate::engine) fn bytes(&mut self, b: &[u8]) {
        let end = self.at + b.len();
        if end > self.buf.len() {
            self.fits = false;
            return;
        }
        self.buf[self.at..end].copy_from_slice(b);
        self.at = end;
    }

    pub(in crate::engine) fn u8(&mut self, v: u8) {
        self.bytes(&[v]);
    }

    pub(in crate::engine) fn u16(&mut self, v: u16) {
        self.bytes(&v.to_be_bytes());
    }

    pub(in crate::engine) fn u32(&mut self, v: u32) {
        self.bytes(&v.to_be_bytes());
    }

    /// A flag, then the value when there is one.
    pub(in crate::engine) fn opt_u32(&mut self, v: Option<u32>) {
        self.u8(v.is_some() as u8);
        self.u32(v.unwrap_or(0));
    }

    /// The bytes written; 0 when some write did not fit (the importer then starts fresh).
    pub(in crate::engine) fn finish(self) -> usize {
        if self.fits {
            self.at
        } else {
            0
        }
    }
}
