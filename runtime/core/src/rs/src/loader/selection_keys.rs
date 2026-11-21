//! The selection's header tables are shared with service callbacks through `CoreApi::keys_of`.
//! The device stores them outside the loader so a callback can read them during an exclusive loader operation.

use crate::plan::TABLE_MAX;
use dkneeds::keys::Keys;
use dkrt_sdk::SEL_MAX;

pub struct SelectionKeys {
    pub(super) tables: [[u8; TABLE_MAX]; SEL_MAX],
    /// Zero when no header table was captured.
    pub(super) table_len: [usize; SEL_MAX],
    /// Only accepted entries contribute keys to the services' numbering.
    pub(super) accepted: [bool; SEL_MAX],
}

impl SelectionKeys {
    pub const fn new() -> Self {
        Self {
            tables: [[0; TABLE_MAX]; SEL_MAX],
            table_len: [0; SEL_MAX],
            accepted: [false; SEL_MAX],
        }
    }

    pub(super) fn set_table(&mut self, at: usize, table: &[u8]) {
        self.tables[at][..table.len()].copy_from_slice(table);
        self.table_len[at] = table.len();
    }

    pub(super) fn table(&self, at: usize) -> &[u8] {
        &self.tables[at][..self.table_len[at]]
    }

    pub(super) fn parse(&self, count: usize) -> [Option<Keys<'_>>; SEL_MAX] {
        let mut mods = [None; SEL_MAX];
        for (at, slot) in mods[..count].iter_mut().enumerate() {
            if self.table_len[at] != 0 {
                *slot = Keys::parse(self.table(at)).ok();
            }
        }
        mods
    }

    pub(super) fn keys_of(&self, at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        let k = at as usize;
        if k >= SEL_MAX || !self.accepted[k] {
            return 0;
        }
        dkrt_abi::keys::keys_of(self.table(k), kind, out)
    }
}

/// References stay within the closure. All callers finish their table access before I/O or a service callback.
pub trait SelectionStorage {
    fn read<T>(&self, f: impl FnOnce(&SelectionKeys) -> T) -> T;
    fn write<T>(&mut self, f: impl FnOnce(&mut SelectionKeys) -> T) -> T;
}

#[cfg(test)]
impl SelectionStorage for SelectionKeys {
    fn read<T>(&self, f: impl FnOnce(&SelectionKeys) -> T) -> T {
        f(self)
    }

    fn write<T>(&mut self, f: impl FnOnce(&mut SelectionKeys) -> T) -> T {
        f(self)
    }
}

/// The device loader's zero-sized handle. The cell is also read directly by the service API.
pub struct RealSelection;

impl RealSelection {
    pub(super) fn keys_of(at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        real::keys_of(at, kind, out)
    }
}

impl SelectionStorage for RealSelection {
    fn read<T>(&self, f: impl FnOnce(&SelectionKeys) -> T) -> T {
        real::read(f)
    }

    fn write<T>(&mut self, f: impl FnOnce(&mut SelectionKeys) -> T) -> T {
        real::write(f)
    }
}

#[cfg(test)]
mod real {
    use super::*;
    use std::sync::Mutex;

    static KEYS: Mutex<SelectionKeys> = Mutex::new(SelectionKeys::new());

    pub(super) fn keys_of(at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        read(|keys| keys.keys_of(at, kind, out))
    }

    pub(super) fn read<T>(f: impl FnOnce(&SelectionKeys) -> T) -> T {
        f(&KEYS.lock().unwrap_or_else(|e| e.into_inner()))
    }

    pub(super) fn write<T>(f: impl FnOnce(&mut SelectionKeys) -> T) -> T {
        f(&mut KEYS.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

#[cfg(not(test))]
mod real {
    use super::*;
    use core::cell::{Ref, RefCell};
    use core::sync::atomic::{compiler_fence, Ordering};

    struct IrqMask(u32);

    impl IrqMask {
        fn new() -> Self {
            let sr = crate::irq_mask();
            compiler_fence(Ordering::SeqCst);
            Self(sr)
        }
    }

    impl Drop for IrqMask {
        fn drop(&mut self) {
            compiler_fence(Ordering::SeqCst);
            crate::irq_restore(self.0);
        }
    }

    struct Cell(RefCell<SelectionKeys>);

    // ColdFire runs the runtime on one CPU and task switches are masked by IrqMask. We change
    // RefCell's borrow flag only under that mask. Shared borrows can coexist across task switches;
    // their checked borrow guards keep the tables immutable until the last reader is finished.
    // The loader serializes its own reads and writes. The service API can also read from
    // another task, so it masks the entire bounded lookup to avoid preemption by a writer.
    // Neither writes nor the service lookup yield or call service code under the mask.
    unsafe impl Sync for Cell {}

    static KEYS: Cell = Cell(RefCell::new(SelectionKeys::new()));

    pub(super) fn keys_of(at: u32, kind: u32, out: &mut [u32; 8]) -> usize {
        let _mask = IrqMask::new();
        KEYS.0.borrow().keys_of(at, kind, out)
    }

    struct Read<'a>(Option<Ref<'a, SelectionKeys>>);

    impl Drop for Read<'_> {
        fn drop(&mut self) {
            let _mask = IrqMask::new();
            drop(self.0.take());
        }
    }

    pub(super) fn read<T>(f: impl FnOnce(&SelectionKeys) -> T) -> T {
        let keys = {
            let _mask = IrqMask::new();
            Read(Some(KEYS.0.borrow()))
        };
        f(keys.0.as_deref().unwrap())
    }

    pub(super) fn write<T>(f: impl FnOnce(&mut SelectionKeys) -> T) -> T {
        let _mask = IrqMask::new();
        let mut keys = KEYS.0.borrow_mut();
        f(&mut keys)
    }
}
