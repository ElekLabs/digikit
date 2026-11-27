use dkrt_abi::dsp::{L2_BYTE_BASE, RELOC_LEN, RELOC_OFFSET, RELOC_WORD};

/// A relocation entry named a place or a value that does not fit: the file is malformed (the package's `E_DSP_FORMAT`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Malformed;

/// A module's DSP image rebased to the range it got: abs24 and abs32 fields of 48-bit instructions, shifted by where the range sits in L2. An entry is 8 bytes of the file's table: the
/// byte offset of the instruction in the image, then `kind << 28 | class << 24 | import index`. Kind 0 is a 24-bit field (pc), kind 1 a 32-bit one (data). Class 0 adds the pc shift,
/// class 1 the byte shift, class 2 the value of an import.
pub struct Relocation<'a> {
    pc_shift: u32,
    byte_shift: u32,
    imports: &'a [u32],
}

impl<'a> Relocation<'a> {
    /// The relocation of an image that lies at byte address `range_base` of the area; `imports` are the values of the file's imports.
    pub fn at(range_base: u32, imports: &'a [u32]) -> Self {
        let byte_shift = range_base - L2_BYTE_BASE;
        Relocation {
            pc_shift: byte_shift / 2,
            byte_shift,
            imports,
        }
    }

    /// Copy `image` into `out` (as long as `image`) and relocate the copy by `table` (entries of the file). `Malformed` when an entry names a place outside the image, an import the file
    /// does not have, or a 24-bit field that does not fit after the shift; `out` is then not usable.
    pub fn place(&self, image: &[u8], out: &mut [u8], table: &[u8]) -> Result<(), Malformed> {
        if out.len() != image.len() {
            return Err(Malformed);
        }
        out.copy_from_slice(image);
        let mut i = 0;
        while i + RELOC_LEN as usize <= table.len() {
            let e = &table[i..i + RELOC_LEN as usize];
            let at = be32(e, RELOC_OFFSET as usize) as usize;
            let w = be32(e, RELOC_WORD as usize);
            let (kind, class, imp) = (w >> 28, (w >> 24) & 15, (w & 0xffff) as usize);
            let add = if class == 0 {
                self.pc_shift
            } else if class == 1 {
                self.byte_shift
            } else {
                *self.imports.get(imp).ok_or(Malformed)?
            };
            let p = out.get_mut(at..at + 6).ok_or(Malformed)?;
            if kind == 0 {
                let v = (((p[2]) as u32) << 16 | le16(&p[4..])).wrapping_add(add);
                if v >= 1 << 24 {
                    return Err(Malformed);
                }
                p[2] = (v >> 16) as u8;
                put_le16(&mut p[4..], v & 0xffff);
            } else {
                let v = (le16(&p[2..]) << 16 | le16(&p[4..])).wrapping_add(add);
                put_le16(&mut p[2..], v >> 16);
                put_le16(&mut p[4..], v & 0xffff);
            }
            i += RELOC_LEN as usize;
        }
        Ok(())
    }
}

fn be32(b: &[u8], at: usize) -> u32 {
    (b[at] as u32) << 24 | (b[at + 1] as u32) << 16 | (b[at + 2] as u32) << 8 | b[at + 3] as u32
}

fn le16(b: &[u8]) -> u32 {
    b[0] as u32 | (b[1] as u32) << 8
}

fn put_le16(b: &mut [u8], v: u32) {
    b[0] = v as u8;
    b[1] = (v >> 8) as u8;
}

#[cfg(test)]
mod tests;
