//! Module ids and the file names of their modules.

/// The longest module id (a module record holds 31 bytes and a NUL).
pub const ID_MAX: usize = 31;

/// A module id (`<author>/<name>`, 1 to `ID_MAX` bytes).
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModuleId {
    bytes: [u8; ID_MAX],
    len: u8,
}

/// The file name of a module in the store: its id with `/` as `-`, then `.dkm` (docs/design/taxonomy.md).
pub struct ModuleFile {
    bytes: [u8; ID_MAX + 4],
    len: usize,
}

impl ModuleFile {
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

impl ModuleId {
    pub const EMPTY: ModuleId = ModuleId {
        bytes: [0; ID_MAX],
        len: 0,
    };

    /// `None` for an id that is empty or longer than `ID_MAX`.
    pub fn new(id: &[u8]) -> Option<ModuleId> {
        if id.is_empty() || id.len() > ID_MAX {
            return None;
        }
        let mut m = ModuleId::EMPTY;
        let mut at = 0;
        while at < id.len() {
            m.bytes[at] = id[at];
            at += 1;
        }
        m.len = id.len() as u8;
        Some(m)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// The module's own name: the part after the author.
    pub fn name(&self) -> &[u8] {
        let id = self.as_bytes();
        let mut at = 0;
        while at < id.len() {
            if id[at] == b'/' {
                return &id[at + 1..];
            }
            at += 1;
        }
        id
    }

    /// FNV-1a (32 bits) of the id: the `name_hash` of the SDK, the id word of `PROJECT_MODULE`.
    pub fn hash(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        let id = self.as_bytes();
        let mut at = 0;
        while at < id.len() {
            h ^= id[at] as u32;
            h = h.wrapping_mul(0x0100_0193);
            at += 1;
        }
        h
    }

    pub fn file(&self) -> ModuleFile {
        let mut f = ModuleFile {
            bytes: [0; ID_MAX + 4],
            len: 0,
        };
        let id = self.as_bytes();
        let mut at = 0;
        while at < id.len() {
            f.bytes[at] = if id[at] == b'/' { b'-' } else { id[at] };
            at += 1;
        }
        f.bytes[at..at + 4].copy_from_slice(b".dkm");
        f.len = at + 4;
        f
    }
}
