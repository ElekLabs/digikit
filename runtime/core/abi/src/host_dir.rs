//! `HostDir`: the host directory (`DKRT_HOST_DIR`), the one resident table of addresses through which host tools find the statics of the system package.
//!
//! The package is a card file that stage 1 places at a different address each boot, so none of its statics has a symbol in the image. The directory is resident: a magic,
//! the number of ids the core that built it knows (`count`) and one device address per id (`HOST_*` of `abi.toml`, `[[host_dir]]`). Stage 1 clears it before it starts the
//! package (`clear`); the package fills each entry at its entry, through the resident table (`HostDirFns::publish`); a core-only boot leaves it empty. A tool reads this one symbol
//! and follows the address of the id it needs (`get`); an empty entry means no package is loaded.
//!
//! Each id has an owner (`owner` of `[[host_dir]]`): the package fills its own statics at its entry, the core fills what it knows (the block it placed the package in) before it
//! starts the package. A `ready` word is written last, once the package has attached: a tool that finds the directory not ready (cleared, or the package still starting) reads it as
//! "no package", never as a table with some entries filled.
//!
//! Ids are append-only (their order is `[[host_dir]]` in `abi.toml`), so the directory grows at its end: a tool built against more ids than the core's `count` gets `None` for the
//! later ones, a core with more ids than the tool knows has entries the tool never asks for. The layout is `repr(C)` words, the same on the device and a host.

use dkrt_sdk::{
    HOST_CORE_OWNED, HOST_COUNT, HOST_DIR_MAGIC, HOST_DIR_READY_MAGIC, HOST_PACKAGE_OWNED,
};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostDir {
    magic: u32,
    count: u32,
    /// `HOST_DIR_READY_MAGIC` once the package has attached; 0 until then.
    ready: u32,
    addr: [u32; HOST_COUNT],
}

/// Does the package fill the entry of `id` (`owner = "package"`)?
pub const fn package_owned(id: u32) -> bool {
    let mut i = 0;
    while i < HOST_PACKAGE_OWNED.len() {
        if HOST_PACKAGE_OWNED[i] == id {
            return true;
        }
        i += 1;
    }
    false
}

/// Does the core fill the entry of `id` (`owner = "core"`)?
pub const fn core_owned(id: u32) -> bool {
    let mut i = 0;
    while i < HOST_CORE_OWNED.len() {
        if HOST_CORE_OWNED[i] == id {
            return true;
        }
        i += 1;
    }
    false
}

impl HostDir {
    /// The directory of a core with no package started: the magic and the count, not ready, every entry empty.
    pub const fn empty() -> HostDir {
        HostDir {
            magic: HOST_DIR_MAGIC,
            count: HOST_COUNT as u32,
            ready: 0,
            addr: [0; HOST_COUNT],
        }
    }

    /// Empty every entry (stage 1, before it starts the package).
    pub fn clear(&mut self) {
        *self = HostDir::empty();
    }

    /// Set the entry of `id` to the device address `addr`; false for an id this core does not know.
    pub fn set(&mut self, id: u32, addr: u32) -> bool {
        if (id as usize) >= HOST_COUNT {
            return false;
        }
        self.addr[id as usize] = addr;
        true
    }

    /// The package has attached and every entry is filled: written last, so a reader that sees it sees whole entries.
    pub fn complete(&mut self) {
        self.ready = HOST_DIR_READY_MAGIC;
    }

    /// Has the package attached (`complete` ran since the last `clear`)?
    pub fn is_ready(&self) -> bool {
        self.ready == HOST_DIR_READY_MAGIC
    }

    /// The address of `id`: `None` for an id past this core's `count` or an entry nobody filled (no package).
    pub fn get(&self, id: u32) -> Option<u32> {
        if id >= self.count.min(HOST_COUNT as u32) {
            return None;
        }
        match self.addr[id as usize] {
            0 => None,
            a => Some(a),
        }
    }
}

#[cfg(test)]
mod tests;
