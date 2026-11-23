//! The host directory of the runtime (`DKRT_HOST_DIR`, `runtime/core/abi.toml` `[[host_dir]]`): how a tool finds a static of the system package.
//!
//! The package is a card file placed at a different address each boot, so none of its statics has a symbol in the image. The resident core keeps one table of their addresses:
//! a magic, the count of ids that core knows and one 32-bit big-endian address per id (the layout of `dkrt_abi::host_dir::HostDir`, the words as the device holds them). A tool
//! reads the bytes of the one resident symbol (`SYM_HOST_DIR`, [`BYTES`] long), asks [`HostDir::address`] for the id it needs and reads the memory there. An empty entry means no
//! package is running (a core-only boot, or no package file); the answer then names the static, never an address of garbage.

use crate::gen_host_dir::{HOST_COUNT, HOST_DIR_MAGIC, HOST_NAMES};

/// The resident symbol of the directory.
pub const SYM_HOST_DIR: &str = "DKRT_HOST_DIR";
/// The bytes of the directory a tool reads: the magic, the count and one word per id this build knows.
pub const BYTES: u64 = 4 * (2 + HOST_COUNT as u64);

/// Why an address could not be found.
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    /// The bytes are not a directory (too short, or another magic): the image has none, or the read went to the wrong place.
    NotADirectory,
    /// The entry is empty: no package is loaded.
    NoPackage(&'static str),
    /// The core that wrote the directory is older than this tool: it has no such id.
    UnknownId(&'static str),
    /// An id this build does not have.
    BadId(u32),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotADirectory => write!(
                f,
                "{SYM_HOST_DIR}: not a host directory (the image has none, or the read was elsewhere)"
            ),
            Error::NoPackage(name) => write!(
                f,
                "no package loaded: the host directory has no address for {name} (the resident core runs alone, or the card has no system package)"
            ),
            Error::UnknownId(name) => write!(
                f,
                "this core does not know {name}: its host directory is older than this tool"
            ),
            Error::BadId(id) => write!(
                f,
                "host directory id {id} is not one of the {HOST_COUNT} this build knows"
            ),
        }
    }
}

/// A directory as read from memory.
pub struct HostDir<'a> {
    words: &'a [u8],
    count: u32,
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

impl<'a> HostDir<'a> {
    /// The directory in `bytes` (what a read of [`SYM_HOST_DIR`] gave).
    pub fn parse(bytes: &'a [u8]) -> Result<HostDir<'a>, Error> {
        if bytes.len() < 8 || word(bytes, 0) != HOST_DIR_MAGIC {
            return Err(Error::NotADirectory);
        }
        let count = word(bytes, 4);
        // a core with more ids than this tool knows wrote more words than were read: only the ones the tool knows are used
        Ok(HostDir {
            words: bytes,
            count,
        })
    }

    /// The address of the static with `id` (`HOST_*` of `gen_host_dir`).
    pub fn address(&self, id: u32) -> Result<u32, Error> {
        let name = *HOST_NAMES.get(id as usize).ok_or(Error::BadId(id))?;
        let at = 8 + 4 * id as usize;
        if id >= self.count || at + 4 > self.words.len() {
            return Err(Error::UnknownId(name));
        }
        match word(self.words, at) {
            0 => Err(Error::NoPackage(name)),
            a => Ok(a),
        }
    }
}
