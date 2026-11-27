//! What the service reads of the firmware: the `MmcFs` object's slot records and the byte of the current slot.
#[cfg(target_arch = "m68k")]
use core::ptr::{addr_of, read_volatile};

/// `MmcFs *f(void)` of the firmware (`cf.project.fn.mmcfs`); 0 on a host build and when the port has no such name.
#[cfg(target_arch = "m68k")]
pub(super) unsafe fn firmware_mmcfs() -> u32 {
    if core::hint::black_box(dkfw::project::mmcfs as usize) == 0 {
        0
    } else {
        dkfw::project::mmcfs()
    }
}
#[cfg(not(target_arch = "m68k"))]
pub(super) unsafe fn firmware_mmcfs() -> u32 {
    0
}

/// The listing record of a slot in the firmware's `MmcFs` object (finding 110, `cf.project.fn.mmcfs` in ports/fw/project.toml; the same on both devices): the record of
/// slot s is at `MmcFs + REC_BASE + s * REC_SIZE`; byte 0 is nonzero when the slot exists (the firmware's own test), the 16 name bytes are at `REC_NAME`, the save stamp
/// (u32) at `REC_STAMP`.
pub(super) const REC_BASE: u32 = 0x10;
pub(super) const REC_SIZE: u32 = 0x1c;
pub(super) const REC_NAME: usize = 8;
pub(super) const REC_STAMP: usize = 0x18;

/// The slot of the current project as the firmware stores it (`cf.project.value.slot_byte`: one byte, -1 for none); `None` on a host build and for no project. The byte holds the
/// last persisted slot, read before the stock project load (ports/fw/project.toml). The extern is declared here and not in `dkfw` (the value is `codegen = false`), and
/// `[object.externs]` of `dks/module.toml` names it, as it names the hook tails: `patchkit pack` makes it the import `dk_fw_<hash>`, which the loader resolves from the
/// port file. The address is 0 only when the port marks the name absent for the firmware (the loader accepts the file and the read is off).
#[cfg(target_arch = "m68k")]
pub(super) fn firmware_slot() -> Option<u32> {
    extern "C" {
        static project_slot_byte: u8;
    }
    // SAFETY: the address is the firmware's variable, resolved by the port file; 0 when the port marks it absent.
    unsafe {
        let at = core::hint::black_box(addr_of!(project_slot_byte) as usize);
        if at == 0 {
            return None;
        }
        let slot = read_volatile(at as *const u8) as i8;
        if slot >= 0 {
            Some(slot as u32)
        } else {
            None
        }
    }
}
#[cfg(not(target_arch = "m68k"))]
pub(super) fn firmware_slot() -> Option<u32> {
    None
}
