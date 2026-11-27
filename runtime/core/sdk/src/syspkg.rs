//! The header of the system package, the card file that holds the runtime's package manager, planner and menu (docs/plans/resident-core-and-system-package.md).
//! One definition for both sides: patchkit writes it in front of the packed body, the resident stage 1 reads it before it places the body. Pure code,
//! `no_std`, no hashing: the hash is the caller's (stage 1 streams the body through the runtime's `sha256`, patchkit through its own); the header only
//! holds the expected value and compares.
//!
//! The header is 48 bytes, big-endian words:
//!   0 `DKSP`   1 format version (`FORMAT`)   2 resident table major << 16 | minor that the package was built against   3 body size in bytes
//!   4..11 sha256 of the body (32 bytes)
//! A package is compatible with a core when the majors are equal and the package's minor is not newer than the core's (the core's table only grows
//! by appending, so a package that wants fewer functions runs on a newer core; one that wants more does not).

pub mod body;

pub const MAGIC: u32 = 0x444b_5350; // "DKSP"
/// The layout of this header. A header of another format is refused, never guessed at.
pub const FORMAT: u32 = 1;
pub const HEADER_BYTES: usize = 48;

/// Why a header was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    /// Fewer than `HEADER_BYTES` bytes.
    Short,
    BadMagic,
    BadFormat,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    table_major: u16,
    table_minor: u16,
    size: u32,
    sha256: [u8; 32],
}

impl Header {
    /// The header of a body of `size` bytes with digest `sha256`, built against resident table `table_major.table_minor`.
    pub fn new(table_major: u16, table_minor: u16, size: u32, sha256: [u8; 32]) -> Header {
        Header {
            table_major,
            table_minor,
            size,
            sha256,
        }
    }

    pub fn encode(&self) -> [u8; HEADER_BYTES] {
        let mut out = [0u8; HEADER_BYTES];
        out[0..4].copy_from_slice(&MAGIC.to_be_bytes());
        out[4..8].copy_from_slice(&FORMAT.to_be_bytes());
        out[8..10].copy_from_slice(&self.table_major.to_be_bytes());
        out[10..12].copy_from_slice(&self.table_minor.to_be_bytes());
        out[12..16].copy_from_slice(&self.size.to_be_bytes());
        out[16..48].copy_from_slice(&self.sha256);
        out
    }

    /// Read a header from the start of `bytes` (more bytes may follow: the body).
    pub fn decode(bytes: &[u8]) -> Result<Header, HeaderError> {
        if bytes.len() < HEADER_BYTES {
            return Err(HeaderError::Short);
        }
        if word(bytes, 0) != MAGIC {
            return Err(HeaderError::BadMagic);
        }
        if word(bytes, 4) != FORMAT {
            return Err(HeaderError::BadFormat);
        }
        let mut sha256 = [0u8; 32];
        sha256.copy_from_slice(&bytes[16..48]);
        let table = word(bytes, 8);
        Ok(Header {
            table_major: (table >> 16) as u16,
            table_minor: (table & 0xffff) as u16,
            size: word(bytes, 12),
            sha256,
        })
    }

    /// Can a core whose resident table is `resident_major.resident_minor` run this package?
    pub fn compatible(&self, resident_major: u16, resident_minor: u16) -> bool {
        crate::table_version::check(
            (self.table_major, self.table_minor),
            (resident_major, resident_minor),
        )
        .is_ok()
    }

    /// Does a body of `len` bytes with digest `sha256` (as the caller computed it) match the header?
    pub fn body_matches(&self, len: u32, sha256: &[u8; 32]) -> bool {
        self.size == len && self.sha256 == *sha256
    }

    pub fn table_major(&self) -> u16 {
        self.table_major
    }

    pub fn table_minor(&self) -> u16 {
        self.table_minor
    }

    /// The body's size in bytes.
    pub fn size(&self) -> u32 {
        self.size
    }

    pub fn sha256(&self) -> &[u8; 32] {
        &self.sha256
    }
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

#[cfg(test)]
mod tests;
