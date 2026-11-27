//! The hooks' record: what the five hooks of the stock project paths wrote since the last tick, and how the tick takes it.
use super::Env;
use core::cell::UnsafeCell;

/// The hooks' record (`repr(C)`, written by `hooks.s`): the slot the setter was called with and whether it was, the slot of a successful real save and whether there was one, and one
/// bit for each slot cleared or pasted over (byte `slot / 8`, bit `slot % 8`). A flag is written after its slot. The tick takes the record with interrupts masked and zeroes it.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct Pending {
    pub slot_val: u32,
    pub slot_flag: u32,
    pub save_slot: u32,
    pub save_flag: u32,
    pub clear: [u8; 16],
    pub paste: [u8; 16],
}

impl Pending {
    pub const fn new() -> Pending {
        Pending {
            slot_val: 0,
            slot_flag: 0,
            save_slot: 0,
            save_flag: 0,
            clear: [0; 16],
            paste: [0; 16],
        }
    }
}

impl Default for Pending {
    fn default() -> Self {
        Self::new()
    }
}

/// What the hooks recorded since the last tick, as plain values (a copy of [`Pending`]).
pub struct HookRecords {
    /// The slot the stock setter was called with.
    pub slot_set: Option<u32>,
    /// The slot of a successful stock save.
    pub saved_slot: Option<u32>,
    /// One bit per cleared slot.
    pub cleared: [u8; 16],
    /// One bit per pasted slot.
    pub pasted: [u8; 16],
}

/// The shared hooks' record: a [`Pending`] in an `UnsafeCell`, because a hook writes it from another task while the UI task holds a shared reference. Every access of the UI task runs
/// with interrupts masked (`Env::irq_mask`), as the hooks' assembler expects. `repr(transparent)`: the layout is that of `Pending` (`hooks.s`).
#[repr(transparent)]
pub struct HookRecord(UnsafeCell<Pending>);

// SAFETY: the record is plain words. The hooks write it in their task; the UI task reads and writes it only with interrupts masked, so the two never overlap.
unsafe impl Sync for HookRecord {}

impl HookRecord {
    pub const fn new(record: Pending) -> HookRecord {
        HookRecord(UnsafeCell::new(record))
    }

    /// Put `from` into this record (UI task, interrupts masked: a hook may write it): the record of a copy of the service that was taken down. Each flag is written after its slot.
    pub fn put_back<E: Env + ?Sized>(&self, env: &E, from: &Pending) {
        let sr = env.irq_mask();
        let hooks = self.0.get();
        // SAFETY: plain words that a hook writes from another task, written here with interrupts masked.
        unsafe {
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).slot_val), from.slot_val);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).slot_flag), from.slot_flag);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).save_slot), from.save_slot);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).save_flag), from.save_flag);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).clear), from.clear);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).paste), from.paste);
        }
        env.irq_restore(sr);
    }

    /// A copy of the record as it is now (UI task, interrupts masked while it is read).
    pub fn read<E: Env + ?Sized>(&self, env: &E) -> Pending {
        let sr = env.irq_mask();
        let hooks = self.0.get();
        // SAFETY: as in `put_back`, read.
        let record = unsafe {
            Pending {
                slot_val: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).slot_val)),
                slot_flag: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).slot_flag)),
                save_slot: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).save_slot)),
                save_flag: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).save_flag)),
                clear: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).clear)),
                paste: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).paste)),
            }
        };
        env.irq_restore(sr);
        record
    }

    /// Read and zero the record (UI task, interrupts masked while it runs).
    pub fn take<E: Env + ?Sized>(&self, env: &E) -> HookRecords {
        let sr = env.irq_mask();
        let hooks = self.0.get();
        // SAFETY: as in `put_back`; the flags and bitmaps are cleared in the same masked section as they are read.
        let records = unsafe {
            let records = HookRecords {
                slot_set: if core::ptr::read_volatile(core::ptr::addr_of!((*hooks).slot_flag)) != 0 {
                    Some(core::ptr::read_volatile(core::ptr::addr_of!((*hooks).slot_val)))
                } else {
                    None
                },
                saved_slot: if core::ptr::read_volatile(core::ptr::addr_of!((*hooks).save_flag)) != 0 {
                    Some(core::ptr::read_volatile(core::ptr::addr_of!((*hooks).save_slot)))
                } else {
                    None
                },
                cleared: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).clear)),
                pasted: core::ptr::read_volatile(core::ptr::addr_of!((*hooks).paste)),
            };
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).slot_flag), 0);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).save_flag), 0);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).clear), [0; 16]);
            core::ptr::write_volatile(core::ptr::addr_of_mut!((*hooks).paste), [0; 16]);
            records
        };
        env.irq_restore(sr);
        records
    }
}

impl Default for HookRecord {
    fn default() -> Self {
        HookRecord::new(Pending::new())
    }
}
