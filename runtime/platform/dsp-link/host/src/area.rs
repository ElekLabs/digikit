use dkrt_abi::{MAX_MODULES, REC_MAX};
use dkrt_sdk::capacity::first_fit;

/// The DSP area (`dyndsp_info`: the byte address of its first byte and its size) and the ranges of the module records that hold a part of it. A range is `len` bytes at an offset
/// from the start of the area; `len` 0 is unused.
pub struct Area {
    base: u32,
    bytes: u32,
    off: [u32; REC_MAX],
    len: [u32; REC_MAX],
}

impl Area {
    /// An area of `bytes` bytes starting at byte address `base`, all free. A device without a DSP has an area of 0 bytes.
    pub const fn new(base: u32, bytes: u32) -> Area {
        Area {
            base,
            bytes,
            off: [0; REC_MAX],
            len: [0; REC_MAX],
        }
    }

    /// The byte address of the first byte of the area: the slot block of `core/dsp-link` lies directly below it.
    pub fn base(&self) -> u32 {
        self.base
    }

    /// Bytes of the area.
    pub fn bytes(&self) -> u32 {
        self.bytes
    }

    /// First fit: the lowest offset whose byte address is `align`-aligned and whose `len` bytes touch no range of a record; `None` when no gap holds it.
    pub fn first_fit(&self, len: u32, align: u32) -> Option<u32> {
        first_fit(self.base, self.bytes, &self.off, &self.len, len, align)
    }

    /// Give record `rec` the range `[off, off + len)`; it replaces the record's range.
    pub fn take(&mut self, rec: usize, off: u32, len: u32) {
        self.off[rec] = off;
        self.len[rec] = len;
    }

    /// Free the range of record `rec`.
    pub fn release(&mut self, rec: usize) {
        self.take(rec, 0, 0);
    }

    /// The byte address where the range of record `rec` starts.
    pub fn range_base(&self, rec: usize) -> u32 {
        self.base + self.off[rec]
    }

    /// The bytes of the range of record `rec` (0: none).
    pub fn range_len(&self, rec: usize) -> u32 {
        self.len[rec]
    }

    /// Free bytes in the area and the largest free range, for `dkrt_status`.
    pub fn stats(&self) -> (u32, u32) {
        if self.bytes == 0 {
            return (0, 0);
        }
        // the free count and the gaps look at the module records only, as the package's status does (`area_stats`)
        let used: u32 = self.len[..MAX_MODULES].iter().sum();
        let mut largest = 0;
        let mut cursor = 0u32;
        loop {
            let mut next = self.bytes;
            for i in 0..MAX_MODULES {
                if self.len[i] != 0 && self.off[i] >= cursor && self.off[i] < next {
                    next = self.off[i];
                }
            }
            if next - cursor > largest {
                largest = next - cursor;
            }
            if next >= self.bytes {
                break;
            }
            let mut len = 0;
            for i in 0..MAX_MODULES {
                if self.len[i] != 0 && self.off[i] == next {
                    len = self.len[i];
                }
            }
            cursor = next + len;
        }
        (self.bytes - used, largest)
    }
}

#[cfg(test)]
mod tests;
