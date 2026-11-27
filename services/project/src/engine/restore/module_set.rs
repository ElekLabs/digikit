//! A list of modules with the store hash of each file.
use crate::engine::restore::*;
use crate::engine::Env;
use dkrt_sdk::SEL_MAX;

mod carry;

/// The most modules of a list: the runtime's module limit.
pub const MODULES_MAX: usize = SEL_MAX;

/// A list of modules with the store hash of each file, in order.
#[derive(Clone, Copy)]
pub struct ModuleSet {
    ids: [ModuleId; MODULES_MAX],
    hashes: [u32; MODULES_MAX],
    count: usize,
}

impl ModuleSet {
    pub const fn new() -> ModuleSet {
        ModuleSet {
            ids: [ModuleId::EMPTY; MODULES_MAX],
            hashes: [0; MODULES_MAX],
            count: 0,
        }
    }

    /// The content modules that are loaded now, in the order of the runtime's module records, each with the store hash of its file.
    pub fn of_loaded<E: Env + ?Sized>(env: &E) -> ModuleSet {
        let mut set = ModuleSet::new();
        let mut record = 0;
        while record < MODULES_MAX {
            if let Some(module) = env.loaded_module(record) {
                set.push(module.id, env.store_hash(module.path.as_bytes()));
            }
            record += 1;
        }
        set
    }

    /// The module list of an image (an id that is too long for a module is left out).
    pub fn of_image(image: &[u8]) -> ModuleSet {
        let mut set = ModuleSet::new();
        for (id, hash) in crate::dkp::modules(image) {
            if let Some(id) = ModuleId::new(id) {
                set.push(id, hash);
            }
        }
        set
    }

    fn push(&mut self, id: ModuleId, hash: u32) {
        if self.count < MODULES_MAX {
            self.ids[self.count] = id;
            self.hashes[self.count] = hash;
            self.count += 1;
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn id(&self, at: usize) -> &ModuleId {
        &self.ids[at]
    }

    pub fn has(&self, id: &ModuleId) -> bool {
        let mut at = 0;
        while at < self.count {
            if self.ids[at] == *id {
                return true;
            }
            at += 1;
        }
        false
    }

    /// Both lists hold the same ids with the same hashes in the same order.
    pub fn same_as(&self, other: &ModuleSet) -> bool {
        if self.count != other.count {
            return false;
        }
        let mut at = 0;
        while at < self.count {
            if self.ids[at] != other.ids[at] || self.hashes[at] != other.hashes[at] {
                return false;
            }
            at += 1;
        }
        true
    }

    /// The list as `dkp::set_modules` takes it: `out[..len]` is filled, the number is returned.
    pub fn list<'a>(&'a self, out: &mut [(&'a [u8], u32); MODULES_MAX]) -> usize {
        let mut at = 0;
        while at < self.count {
            out[at] = (self.ids[at].as_bytes(), self.hashes[at]);
            at += 1;
        }
        self.count
    }
}

impl Default for ModuleSet {
    fn default() -> Self {
        Self::new()
    }
}
