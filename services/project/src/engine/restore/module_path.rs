//! The store path of a module file.
use dkrt_sdk::SEL_PATH;

/// The store path of a module file is at most 63 bytes.
pub const MODULE_PATH_MAX: usize = 63;

/// A store path of a module file.
#[derive(Clone, Copy)]
pub struct ModulePath {
    bytes: [u8; MODULE_PATH_MAX],
    len: u8,
}

impl ModulePath {
    pub const EMPTY: ModulePath = ModulePath {
        bytes: [0; MODULE_PATH_MAX],
        len: 0,
    };

    /// `None` for a path longer than `MODULE_PATH_MAX`.
    pub fn new(path: &[u8]) -> Option<ModulePath> {
        if path.len() > MODULE_PATH_MAX {
            return None;
        }
        let mut p = ModulePath::EMPTY;
        let mut at = 0;
        while at < path.len() {
            p.bytes[at] = path[at];
            at += 1;
        }
        p.len = path.len() as u8;
        Some(p)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }

    /// Can an entry of the saved selection hold this path (`SEL_PATH` bytes with the NUL)? The live load takes paths up to `MODULE_PATH_MAX`.
    pub fn fits_selection(&self) -> bool {
        self.len > 0 && (self.len as usize) < SEL_PATH
    }
}
