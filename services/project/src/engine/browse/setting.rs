//! `dk_project_setting`: one saved setting of a module as `dk_project_settings` hands it out, and the page reader over a `.dkp` image. The layout is the C struct of
//! `<dk/platform/project.h>` (explicit padding, no implicit one).
use super::super::restore::ID_MAX;
use super::super::{bytes_equal, restore::ModuleId};
use crate::dkp;
use dkrt_sdk::settings::KEY_MAX;

/// The bytes of a value that an entry carries.
pub const VALUE_HEAD: usize = 64;

/// One saved setting of a module.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct ProjectSetting {
    pub key_len: u8,
    /// The value bytes in `head` (the value's length, at most `VALUE_HEAD`).
    pub head_len: u8,
    /// The full length of the value.
    pub value_len: u16,
    /// The key without the owner's `<author>/<name>/` prefix, as the module wrote it.
    pub key: [u8; KEY_MAX],
    /// The first bytes of the value.
    pub head: [u8; VALUE_HEAD],
    pub pad: u8,
}

// the C header <dk/platform/project.h> says 47 key bytes, 64 value bytes and 116 bytes in all
const _: () = assert!(KEY_MAX == 47);
const _: () = assert!(core::mem::size_of::<ProjectSetting>() == 116);
const _: () = assert!(core::mem::offset_of!(ProjectSetting, value_len) == 2);
const _: () = assert!(core::mem::offset_of!(ProjectSetting, key) == 4);
const _: () = assert!(core::mem::offset_of!(ProjectSetting, head) == 51);

impl ProjectSetting {
    pub const EMPTY: ProjectSetting = ProjectSetting {
        key_len: 0,
        head_len: 0,
        value_len: 0,
        key: [0; KEY_MAX],
        head: [0; VALUE_HEAD],
        pad: 0,
    };

    fn set(&mut self, owner: &[u8], key: &[u8], value: &[u8]) {
        // the stored key is `<owner>/<key>`
        let bare = if key.len() > owner.len() + 1
            && bytes_equal(&key[..owner.len()], owner)
            && key[owner.len()] == b'/'
        {
            &key[owner.len() + 1..]
        } else {
            key
        };
        *self = ProjectSetting::EMPTY;
        let n = bare.len().min(KEY_MAX);
        self.key_len = n as u8;
        let mut i = 0;
        while i < n {
            self.key[i] = bare[i];
            i += 1;
        }
        self.value_len = value.len().min(u16::MAX as usize) as u16;
        let h = value.len().min(VALUE_HEAD);
        self.head_len = h as u8;
        i = 0;
        while i < h {
            self.head[i] = value[i];
            i += 1;
        }
    }
}

/// What `read_page` found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    /// The image has no such module.
    NoModule,
    /// `count` entries written; `more` when the module has entries after them.
    Entries { count: usize, more: bool },
}

/// The entries of `module` (the tag-2 section of that owner) from the `cursor`th on, into `out`. `NoModule` when the module list of the image does not hold the id.
pub fn read_page(image: &[u8], module: &[u8], cursor: u32, out: &mut [ProjectSetting]) -> Page {
    if module.len() > ID_MAX || ModuleId::new(module).is_none() {
        return Page::NoModule;
    }
    let mut listed = false;
    for (id, _) in dkp::modules(image) {
        if bytes_equal(id, module) {
            listed = true;
        }
    }
    if !listed {
        return Page::NoModule;
    }
    let mut index = 0u32;
    let mut count = 0;
    for (owner, key, value) in dkp::entries(image) {
        if !bytes_equal(owner, module) {
            continue;
        }
        if index >= cursor {
            if count == out.len() {
                return Page::Entries { count, more: true };
            }
            out[count].set(owner, key, value);
            count += 1;
        }
        index += 1;
    }
    Page::Entries { count, more: false }
}
