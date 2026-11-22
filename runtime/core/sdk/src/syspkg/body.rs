//! The body of the system package (the bytes after the 48-byte `Header`): the linked code and data of the package artifact, ready to be copied to its place and relocated.
//! One definition for both sides: `patchkit syspkg build` writes it, the resident stage 1 places it. Pure code, `no_std`.
//!
//! The body is a 32-byte header, the image, and the relocation table, in this order; all integers are big-endian words:
//!   0 entry: offset of `dkpkg_entry` in the image   1 image bytes (code and data, a multiple of 4)   2 bss bytes (zeroed after the image, not in the body)
//!   3 relocation count   4 alignment of the image start in memory (a power of two, at most 16)   5..7 zero
//! The image starts at body offset 32 (`HEADER_BYTES`), the relocation table at `32 + image bytes`: one word per relocation, the image offset of a word that holds
//! an address inside the package. The package is linked at address 0; placing it at `base` adds `base` to each such word (`relocate`). The package holds no other
//! address: it reaches the resident core through the table it is given and nothing else, so no relocation names a symbol.

/// Bytes of the body header; the image follows it.
pub const HEADER_BYTES: usize = 32;

/// Why a body was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyError {
    /// Fewer bytes than the header and the sizes it names.
    Short,
    /// The alignment is not a power of two of at most 16, the image is not a whole number of words, or the entry is outside the image.
    BadHeader,
    /// A relocation names a word that is not wholly inside the image.
    BadReloc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BodyHeader {
    pub entry: u32,
    pub image: u32,
    pub bss: u32,
    pub relocs: u32,
    pub align: u32,
}

impl BodyHeader {
    pub fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        for (i, w) in [self.entry, self.image, self.bss, self.relocs, self.align]
            .iter()
            .enumerate()
        {
            out[4 * i..4 * i + 4].copy_from_slice(&w.to_be_bytes());
        }
        out
    }

    /// Read the header of `body` (the whole body: its length is checked against the sizes the header names).
    pub fn decode(body: &[u8]) -> Result<BodyHeader, BodyError> {
        BodyHeader::peek(body, body.len() as u32)
    }

    /// Read the header from `head`, the first bytes of a body that is `body_len` bytes long (stage 1 knows the body's size from the file header before it has read all
    /// of it): the same checks as `decode`, with `body_len` in place of the length of the bytes.
    pub fn peek(head: &[u8], body_len: u32) -> Result<BodyHeader, BodyError> {
        if head.len() < HEADER_BYTES {
            return Err(BodyError::Short);
        }
        let h = BodyHeader {
            entry: word(head, 0),
            image: word(head, 4),
            bss: word(head, 8),
            relocs: word(head, 12),
            align: word(head, 16),
        };
        if !h.align.is_power_of_two()
            || h.align > 16
            || h.image % 4 != 0
            || h.entry >= h.image
            || h.entry % 2 != 0
        {
            return Err(BodyError::BadHeader);
        }
        if body_len as u64 != h.bytes() {
            return Err(BodyError::Short);
        }
        Ok(h)
    }

    /// Bytes of the whole body: header, image and relocation table.
    pub fn bytes(&self) -> u64 {
        HEADER_BYTES as u64 + self.image as u64 + 4 * self.relocs as u64
    }

    /// Bytes the package takes in memory: the image and the bss after it.
    pub fn memory(&self) -> u64 {
        self.image as u64 + self.bss as u64
    }

    /// Bytes of memory a body needs to be placed and started in one block: the body as it is read (header, image, relocation table), where the image is then relocated in
    /// place and the bss is zeroed after the image, over the relocation table that has done its work.
    pub fn placed_bytes(&self) -> u64 {
        HEADER_BYTES as u64 + self.image as u64 + (4 * self.relocs as u64).max(self.bss as u64)
    }

    /// The relocation table of `body` (after `decode`).
    pub fn reloc_table<'a>(&self, body: &'a [u8]) -> &'a [u8] {
        &body[HEADER_BYTES + self.image as usize..]
    }
}

/// Add `base` to the word of `image` at each offset of `table` (big-endian words): the package linked at 0 now runs at `base`. Every word must lie wholly inside the image.
pub fn relocate(image: &mut [u8], table: &[u8], base: u32) -> Result<(), BodyError> {
    let mut at = 0;
    while at + 4 <= table.len() {
        let off = word(table, at) as usize;
        if off % 2 != 0 || off.checked_add(4).map_or(true, |end| end > image.len()) {
            return Err(BodyError::BadReloc);
        }
        let v = word(image, off).wrapping_add(base);
        image[off..off + 4].copy_from_slice(&v.to_be_bytes());
        at += 4;
    }
    Ok(())
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[cfg(test)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec::Vec;

    fn body(image: &[u8], relocs: &[u32], bss: u32) -> Vec<u8> {
        let h = BodyHeader {
            entry: 2,
            image: image.len() as u32,
            bss,
            relocs: relocs.len() as u32,
            align: 4,
        };
        let mut b = h.encode().to_vec();
        b.extend_from_slice(image);
        for r in relocs {
            b.extend_from_slice(&r.to_be_bytes());
        }
        b
    }

    #[test]
    fn round_trip_and_sizes() {
        let b = body(&[0; 16], &[4, 8], 40);
        let h = BodyHeader::decode(&b).unwrap();
        assert_eq!(
            (h.entry, h.image, h.bss, h.relocs, h.align),
            (2, 16, 40, 2, 4)
        );
        assert_eq!((h.bytes(), h.memory()), (b.len() as u64, 56));
        assert_eq!(h.reloc_table(&b), &[0, 0, 0, 4, 0, 0, 0, 8]);
    }

    #[test]
    fn the_header_is_read_from_the_first_bytes_of_a_body_of_known_length() {
        let b = body(&[0; 16], &[4, 8], 40);
        assert_eq!(
            BodyHeader::peek(&b[..HEADER_BYTES], b.len() as u32),
            BodyHeader::decode(&b)
        );
        assert_eq!(
            BodyHeader::peek(&b[..HEADER_BYTES - 1], b.len() as u32),
            Err(BodyError::Short)
        );
        assert_eq!(
            BodyHeader::peek(&b[..HEADER_BYTES], b.len() as u32 + 4),
            Err(BodyError::Short)
        );
    }

    #[test]
    fn placement_needs_the_larger_of_the_relocation_table_and_the_bss_after_the_image() {
        let h = |bss, relocs| BodyHeader {
            entry: 2,
            image: 16,
            bss,
            relocs,
            align: 4,
        };
        assert_eq!(h(8, 4).placed_bytes(), 32 + 16 + 16);
        assert_eq!(h(100, 4).placed_bytes(), 32 + 16 + 100);
        assert_eq!(h(0, 0).placed_bytes(), 32 + 16);
        // the body itself always fits
        assert!(h(0, 4).placed_bytes() >= h(0, 4).bytes());
    }

    #[test]
    fn a_header_that_does_not_fit_is_refused() {
        let b = body(&[0; 16], &[4], 0);
        assert_eq!(BodyHeader::decode(&b[..10]), Err(BodyError::Short));
        assert_eq!(BodyHeader::decode(&b[..b.len() - 1]), Err(BodyError::Short));
        let mut bad = b.clone();
        bad[19] = 3; // alignment 3
        assert_eq!(BodyHeader::decode(&bad), Err(BodyError::BadHeader));
        let mut bad = b.clone();
        bad[3] = 16; // the entry is at the end of the image
        assert_eq!(BodyHeader::decode(&bad), Err(BodyError::BadHeader));
    }

    #[test]
    fn relocation_adds_the_base_to_each_named_word() {
        let mut image = [0u8; 12];
        image[0..4].copy_from_slice(&0x10u32.to_be_bytes());
        image[8..12].copy_from_slice(&0x24u32.to_be_bytes());
        relocate(&mut image, &[0, 0, 0, 0, 0, 0, 0, 8], 0x1000).unwrap();
        assert_eq!(word(&image, 0), 0x1010);
        assert_eq!(word(&image, 4), 0);
        assert_eq!(word(&image, 8), 0x1024);
    }

    #[test]
    fn a_relocation_outside_the_image_is_refused() {
        let mut image = [0u8; 8];
        assert_eq!(
            relocate(&mut image, &8u32.to_be_bytes(), 1),
            Err(BodyError::BadReloc)
        );
        assert_eq!(
            relocate(&mut image, &6u32.to_be_bytes(), 1),
            Err(BodyError::BadReloc)
        );
        assert_eq!(image, [0u8; 8]);
    }
}
