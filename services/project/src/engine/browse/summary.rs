//! `dk_project_summary`: what a project's `.dkp` holds, as `dk_project_get` hands it out. The layout is the C struct of `<dk/platform/project.h>` (explicit padding, no implicit one).
use super::super::restore::{ModuleId, ID_MAX, MODULES_MAX};
use super::super::{bytes_equal, Env};
use crate::dkp;

/// `ProjectModule::state`: not known (the card was not looked at).
pub const MODULE_UNKNOWN: u8 = 0;
/// The module is loaded now.
pub const MODULE_LOADED: u8 = 1;

/// One module of the project's module list.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProjectModule {
    pub id_len: u8,
    pub id: [u8; ID_MAX],
    /// The store hash of the module file at the project's last save.
    pub hash: u32,
    /// The number of settings entries the project holds for this module (at most 255).
    pub settings: u8,
    pub state: u8,
    pub pad: [u8; 2],
}

impl ProjectModule {
    const EMPTY: ProjectModule = ProjectModule {
        id_len: 0,
        id: [0; ID_MAX],
        hash: 0,
        settings: 0,
        state: MODULE_UNKNOWN,
        pad: [0; 2],
    };
}

/// The header and the module list of one project.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProjectSummary {
    pub slot: u8,
    pub name_len: u8,
    pub name: [u8; 16],
    pub pad: [u8; 2],
    /// The stock save stamp of the file.
    pub stamp: u32,
    /// The write counter of the file (0 for a project with no file).
    pub counter: u32,
    pub module_count: u8,
    pub pad2: [u8; 3],
    pub modules: [ProjectModule; MODULES_MAX],
}

// the C header <dk/platform/project.h> says 8 modules and 31 id bytes
const _: () = assert!(MODULES_MAX == 8 && ID_MAX == 31);
const _: () = assert!(core::mem::size_of::<ProjectModule>() == 40);
const _: () = assert!(core::mem::offset_of!(ProjectModule, hash) == 32);
const _: () = assert!(core::mem::offset_of!(ProjectModule, settings) == 36);
const _: () = assert!(core::mem::offset_of!(ProjectSummary, stamp) == 20);
const _: () = assert!(core::mem::offset_of!(ProjectSummary, module_count) == 28);
const _: () = assert!(core::mem::offset_of!(ProjectSummary, modules) == 32);
const _: () = assert!(core::mem::size_of::<ProjectSummary>() == 32 + 40 * MODULES_MAX);

impl ProjectSummary {
    /// All zero: the state of a service that starts.
    pub const fn new() -> ProjectSummary {
        ProjectSummary {
            slot: 0,
            name_len: 0,
            name: [0; 16],
            pad: [0; 2],
            stamp: 0,
            counter: 0,
            module_count: 0,
            pad2: [0; 3],
            modules: [ProjectModule::EMPTY; MODULES_MAX],
        }
    }

    /// Read the summary from the bytes of a `.dkp` image that passed `dkp::check` (or is the live image): the header, the module list and the number of settings entries of each module.
    pub fn read_image(&mut self, image: &[u8], slot: u32, counter: u32) {
        let head = dkp::head_of(image);
        self.slot = slot as u8;
        self.name_len = head.name_len.min(16);
        self.name = head.name;
        self.stamp = head.stamp;
        self.counter = counter;
        self.module_count = 0;
        for (id, hash) in dkp::modules(image) {
            let at = self.module_count as usize;
            if at >= MODULES_MAX || ModuleId::new(id).is_none() {
                continue;
            }
            let module = &mut self.modules[at];
            *module = ProjectModule::EMPTY;
            module.id_len = id.len() as u8;
            let mut i = 0;
            while i < id.len() {
                module.id[i] = id[i];
                i += 1;
            }
            module.hash = hash;
            self.module_count += 1;
        }
        let mut at = self.module_count as usize;
        while at < MODULES_MAX {
            self.modules[at] = ProjectModule::EMPTY;
            at += 1;
        }
        for (owner, _key, _value) in dkp::entries(image) {
            let mut at = 0;
            while at < self.module_count as usize {
                let module = &mut self.modules[at];
                if bytes_equal(&module.id[..module.id_len as usize], owner) {
                    module.settings = module.settings.saturating_add(1);
                }
                at += 1;
            }
        }
    }

    /// Mark the modules that are loaded now (`MODULE_LOADED`), the others `MODULE_UNKNOWN`: the runtime's module records, no card access.
    pub fn mark_loaded<E: Env + ?Sized>(&mut self, env: &E) {
        let mut at = 0;
        while at < self.module_count as usize {
            let module = &mut self.modules[at];
            module.state = MODULE_UNKNOWN;
            let mut record = 0;
            while record < MODULES_MAX {
                if let Some(loaded) = env.loaded_module(record) {
                    if bytes_equal(loaded.id.as_bytes(), &module.id[..module.id_len as usize]) {
                        module.state = MODULE_LOADED;
                    }
                }
                record += 1;
            }
            at += 1;
        }
    }
}

impl Default for ProjectSummary {
    fn default() -> Self {
        Self::new()
    }
}
