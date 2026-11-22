//! Stage 1: the resident loader of the system package (docs/plans/resident-core-and-system-package.md, sections 3 and 5). It finds the package on the
//! card, checks its header and its hash while it streams the body through a chunk buffer (the package is larger than the staging buffer), and hands
//! the bytes to a `Placer`. Its result is the `PackageStatus` that `level::decide` takes: a package that is missing, incompatible or damaged is a card
//! state, never a crash.
//!
//! One owner object per boot step: `Stage1` holds the source, the placer and the chunk buffer; `load` is the one verb. The header type is the shared
//! `dkrt_sdk::syspkg::Header` (patchkit writes it with the same type).
//!
//! - `source`: where the bytes come from (`PackageSource`), and the store implementation;
//! - `placer`: where the bytes go (`Placer`); the arena-top placement joins after RC2s-1 S7.
//! - `load`: the steps.

mod load;
mod placer;
mod source;

pub use placer::Placer;
pub use source::{PackageSource, StoreSource};

use crate::level::PackageStatus;

/// The system package's path in the store (the root `ROOT_STORE`, a NUL-terminated key as `store::find` takes it).
pub const PACKAGE_PATH: &[u8] = b"digikit/system.dksp\0";

/// The resident table version this core has (`major`, `minor`): a package is compatible when the majors are equal and its minor is not newer. A constant
/// until the resident table exists.
pub const RESIDENT_TABLE_MAJOR: u16 = 1;
pub const RESIDENT_TABLE_MINOR: u16 = 1;

/// The loader. `chunk` is the buffer the file is read through: its length is a multiple of 512 (the sector the source reads in), at least 512 and
/// 16-byte aligned on a device (the staging buffer).
pub struct Stage1<'a, S: PackageSource, P: Placer> {
    source: S,
    placer: P,
    chunk: &'a mut [u8],
}

impl<'a, S: PackageSource, P: Placer> Stage1<'a, S, P> {
    pub fn new(source: S, placer: P, chunk: &'a mut [u8]) -> Self {
        assert!(chunk.len() >= 512 && chunk.len() % 512 == 0);
        Stage1 { source, placer, chunk }
    }

    /// The placer, after `load` (what it holds is the package when the status was `Ok`).
    pub fn placer(&self) -> &P {
        &self.placer
    }
}

#[cfg(test)]
mod tests;
