//! `HostDir`: the host directory (`DKRT_HOST_DIR`), the one resident table of addresses through which host tools find the statics of the system package.
//!
//! The package is a card file that stage 1 places at a different address each boot, so none of its statics has a symbol in the image. The directory is resident: a magic,
//! the number of ids the core that built it knows (`count`) and one device address per id (`HOST_*` of `abi.toml`, `[[host_dir]]`). Stage 1 clears it before it starts the
//! package (`clear`); the package fills each entry at its entry, through the resident table (`HostDirFns::publish`); a core-only boot leaves it empty. A tool reads this one symbol
//! and follows the address of the id it needs (`get`); an empty entry means no package is loaded.
//!
//! Ids are append-only (their order is `[[host_dir]]` in `abi.toml`), so the directory grows at its end: a tool built against more ids than the core's `count` gets `None` for the
//! later ones, a core with more ids than the tool knows has entries the tool never asks for. The layout is `repr(C)` words, the same on the device and a host.

use dkrt_sdk::{HOST_COUNT, HOST_DIR_MAGIC};

#[repr(C)]
#[derive(Clone, Copy)]
pub struct HostDir {
    magic: u32,
    count: u32,
    addr: [u32; HOST_COUNT],
}

impl HostDir {
    /// The directory of a core with no package started: the magic and the count, every entry empty.
    pub const fn empty() -> HostDir {
        HostDir {
            magic: HOST_DIR_MAGIC,
            count: HOST_COUNT as u32,
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
