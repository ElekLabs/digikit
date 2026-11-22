//! Stage 1: the resident loader of the system package (docs/plans/resident-core-and-system-package.md, sections 3 and 5). It finds the package on the
//! card, checks its header and its hash while it streams the body through a chunk buffer (the package is larger than the staging buffer), and hands
//! the bytes to a `Placer`. Its result is the `PackageStatus` that `level::decide` takes: a package that is missing, incompatible or damaged is a card
//! state, never a crash.
//!
//! One owner object per boot step: `Stage1` holds the source, the placer and the chunk buffer; `load` is the one verb. The header type is the shared
//! `dkrt_sdk::syspkg::Header` (patchkit writes it with the same type).
//!
//! - `source`: where the bytes come from (`PackageSource`), and the store implementation;
//! - `placer`: where the bytes go (`Placer`), and `arena_placer`, the device's: a block at the arena top, relocated in place;
//! - `load`: the steps;
//! - `start`: the table check before the entry and the call of the entry.

mod arena_placer;
mod load;
mod placer;
mod source;
mod start;

pub use arena_placer::ArenaPlacer;
pub use placer::Placer;
pub use source::{PackageSource, StoreSource};
pub use start::{accept_table, log_refusal, start};

use crate::level::PackageStatus;
use dkrt_abi::dev_addr::DevAddr;

/// The system package's path in the store (the root `ROOT_STORE`, a NUL-terminated key as `store::find` takes it).
pub const PACKAGE_PATH: &[u8] = b"digikit/system.dksp\0";

/// The resident table version this core has: a package is compatible when the majors are equal and its minor is not newer (`dkrt_abi::resident::check`
/// decides the same for a table; `syspkg::Header::compatible` is that rule for the header, pinned equal by a test in `resident/tests/check.rs`).
pub use dkrt_abi::resident::{RESIDENT_MAJOR as RESIDENT_TABLE_MAJOR, RESIDENT_MINOR as RESIDENT_TABLE_MINOR};

/// The loader. `chunk` is the buffer the file is read through: its length is a multiple of 512 (the sector the source reads in), at least 512 and
/// 16-byte aligned on a device (the staging buffer).
pub struct Stage1<'a, S: PackageSource, P: Placer> {
    source: S,
    placer: P,
    chunk: &'a mut [u8],
    /// After a `load` that gave `Ok`: the package's entry and the resident table minor it was built against.
    placed: Option<(DevAddr, u16)>,
}

impl<'a, S: PackageSource, P: Placer> Stage1<'a, S, P> {
    pub fn new(source: S, placer: P, chunk: &'a mut [u8]) -> Self {
        assert!(chunk.len() >= 512 && chunk.len() % 512 == 0);
        Stage1 { source, placer, chunk, placed: None }
    }

    /// The entry of the placed package and the resident table minor it was built against: `Some` after a `load` that gave `PackageStatus::Ok`.
    pub fn placed(&self) -> Option<(DevAddr, u16)> {
        self.placed
    }

    /// Give the placed package back (its table was refused after all): the block returns to the arena and `placed` is empty.
    pub fn abandon(&mut self) {
        self.placer.abandon();
        self.placed = None;
    }

    /// The placer, after `load` (what it holds is the package when the status was `Ok`).
    pub fn placer(&self) -> &P {
        &self.placer
    }
}

#[cfg(test)]
mod tests;
