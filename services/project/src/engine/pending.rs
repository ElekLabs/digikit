//! The hooks' record: what the five hooks of the stock project paths wrote since the last tick, and how the tick takes it.
use super::Env;

/// The hooks' record (`repr(C)`, written by `hooks.s`): the slot the setter was called with and whether it was, the slot of a successful real save and whether there was one, and one
/// bit for each slot cleared or pasted over (byte `slot / 8`, bit `slot % 8`). A flag is written after its slot. The tick takes the record with interrupts masked and zeroes it.
#[repr(C)]
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

/// Read and zero the hooks' record (UI task, interrupts masked while it runs).
pub fn take_hook_records<E: Env + ?Sized>(env: &E, pending: &Pending) -> HookRecords {
    let sr = env.irq_mask();
    // SAFETY: `pending` is the hooks' record: plain words that a hook writes from another task, read and cleared here with interrupts masked.
    let records = unsafe {
        let hooks = pending as *const Pending as *mut Pending;
        let records = HookRecords {
            slot_set: if core::ptr::read_volatile(&(*hooks).slot_flag) != 0 {
                Some(core::ptr::read_volatile(&(*hooks).slot_val))
            } else {
                None
            },
            saved_slot: if core::ptr::read_volatile(&(*hooks).save_flag) != 0 {
                Some(core::ptr::read_volatile(&(*hooks).save_slot))
            } else {
                None
            },
            cleared: core::ptr::read_volatile(&(*hooks).clear),
            pasted: core::ptr::read_volatile(&(*hooks).paste),
        };
        core::ptr::write_volatile(&mut (*hooks).slot_flag, 0);
        core::ptr::write_volatile(&mut (*hooks).save_flag, 0);
        core::ptr::write_volatile(&mut (*hooks).clear, [0; 16]);
        core::ptr::write_volatile(&mut (*hooks).paste, [0; 16]);
        records
    };
    env.irq_restore(sr);
    records
}
