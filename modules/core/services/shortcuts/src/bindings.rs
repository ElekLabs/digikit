//! `BindingTable`: the key bindings of all modules. A binding is two logical keys (hold the first, press the second) and the module's callbacks. The first owner of a key
//! pair keeps its binding; a later binding of the same pair stays in the table but disabled, so `remove(owner)` of the first owner lets the next one take over.
use core::ffi::c_void;

/// A module's callback: `on` and `off` of a shortcut get the `arg` the module gave. Module code, called with the table's owner as the running source later on.
pub type Callback = unsafe extern "C" fn(*mut c_void);

/// Bindings the table holds, all modules together.
pub const MAX_BINDINGS: usize = 16;
/// Bytes of an id (with the NUL) and of a label.
pub const ID_BYTES: usize = 16;
pub const LABEL_BYTES: usize = 24;

/// One binding of the table.
#[derive(Clone, Copy)]
pub struct Binding {
    /// The module record index + 1; 0: the entry is free.
    pub owner: u8,
    /// Hold `keys[0]`, press `keys[1]`.
    pub keys: [u8; 2],
    /// The binding does nothing: another owner's binding of the same keys is enabled, or `inactive`.
    pub disabled: bool,
    /// A key of the binding has no panel code on this device. Such a binding is never the keeper of its key pair and stays disabled for good.
    pub inactive: bool,
    pub on: Option<Callback>,
    pub off: Option<Callback>,
    pub arg: *mut c_void,
    /// NUL padded: for a later list and help page.
    pub id: [u8; ID_BYTES],
    pub label: [u8; LABEL_BYTES],
}

impl Binding {
    pub const fn free() -> Binding {
        Binding {
            owner: 0,
            keys: [0; 2],
            disabled: false,
            inactive: false,
            on: None,
            off: None,
            arg: core::ptr::null_mut(),
            id: [0; ID_BYTES],
            label: [0; LABEL_BYTES],
        }
    }
}

/// What `BindingTable::add` did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Added {
    /// The binding is enabled.
    Enabled,
    /// Another owner keeps the same keys: the binding is in the table but disabled. `owner` is that owner.
    Disabled { owner: u8 },
    /// A key has no code on this device: the binding is in the table, disabled, and keeps nothing.
    Inactive,
    /// No room.
    Full,
}

/// The bindings, in the order they were added. All-zero bytes are the empty table.
pub struct BindingTable {
    items: [Binding; MAX_BINDINGS],
}

impl BindingTable {
    pub const fn new() -> BindingTable {
        BindingTable {
            items: [Binding::free(); MAX_BINDINGS],
        }
    }

    /// The enabled binding of the same keys, as the index.
    fn keeper(&self, keys: [u8; 2]) -> Option<usize> {
        let mut i = 0;
        while i < MAX_BINDINGS {
            let b = &self.items[i];
            if b.owner != 0 && !b.disabled && b.keys[0] == keys[0] && b.keys[1] == keys[1] {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// Add a binding (`b.owner` is not 0). The first owner of the key pair keeps it: a later binding of the same pair is added disabled.
    pub fn add(&mut self, b: Binding) -> Added {
        let keeper = if b.inactive {
            None
        } else {
            self.keeper(b.keys)
        };
        let mut i = 0;
        while i < MAX_BINDINGS {
            if self.items[i].owner == 0 {
                self.items[i] = b;
                if b.inactive {
                    self.items[i].disabled = true;
                    return Added::Inactive;
                }
                return match keeper {
                    Some(k) => {
                        self.items[i].disabled = true;
                        Added::Disabled {
                            owner: self.items[k].owner,
                        }
                    }
                    None => {
                        self.items[i].disabled = false;
                        Added::Enabled
                    }
                };
            }
            i += 1;
        }
        Added::Full
    }

    /// Drop every binding of `owner`. A disabled binding whose keeper is gone becomes enabled (the earliest one of each key pair).
    pub fn remove(&mut self, owner: u8) {
        let mut i = 0;
        while i < MAX_BINDINGS {
            if self.items[i].owner == owner {
                self.items[i] = Binding::free();
            }
            i += 1;
        }
        i = 0;
        while i < MAX_BINDINGS {
            if self.items[i].owner != 0 && self.items[i].disabled && !self.items[i].inactive {
                let keys = self.items[i].keys;
                if self.keeper(keys).is_none() {
                    self.items[i].disabled = false;
                }
            }
            i += 1;
        }
    }

    /// The enabled binding that `pressed` completes while `held(key)` is true for its first key: the index into the table.
    pub fn completed(&self, pressed: u32, held: impl Fn(u32) -> bool) -> Option<usize> {
        let mut i = 0;
        while i < MAX_BINDINGS {
            let b = &self.items[i];
            if b.owner != 0
                && !b.disabled
                && u32::from(b.keys[1]) == pressed
                && held(u32::from(b.keys[0]))
            {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn get(&self, index: usize) -> Option<&Binding> {
        self.items.get(index).filter(|b| b.owner != 0)
    }

    /// Bindings in the table (disabled ones too).
    pub fn len(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < MAX_BINDINGS {
            if self.items[i].owner != 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// Bindings of `owner` (0 is no owner: a free entry is no binding).
    pub fn count_of(&self, owner: u8) -> usize {
        if owner == 0 {
            return 0;
        }
        let mut n = 0;
        let mut i = 0;
        while i < MAX_BINDINGS {
            if self.items[i].owner == owner {
                n += 1;
            }
            i += 1;
        }
        n
    }
}
