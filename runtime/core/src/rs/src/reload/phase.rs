//! The reload phase and the count of live jobs: the words a service and the menus read.

/// While a reload runs: the owner of the module being reloaded (module record index + 1), else 0. From the checks of the new file until the end of the job
/// (`CoreApi::live_op`). The reload job sets and clears it; a service reads it where the module's own rows must not count against the new file (the
/// checks of its types and rows) and where its teardown differs from a plain unload (the placeholder rows). A load and an unload leave it 0: a plain unload
/// writes gap rows, and a load has no rows of its own to leave out.
static mut PHASE: u32 = 0;

/// The reload phase now.
pub fn phase() -> u32 {
    unsafe { core::ptr::read_volatile(core::ptr::addr_of!(PHASE)) }
}

/// Start the reload phase of module `owner` (record index + 1), or end it (0). Only the reload job calls it.
pub(crate) fn set_phase(owner: u8) {
    unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(PHASE), owner as u32) }
}

/// A live job changed the module set: the counter that tells a host and the menus their lists of modules and machines are out of date (`DkrtBootInfo.reloads`).
pub(crate) unsafe fn note_reloaded() {
    let b = crate::boot::info();
    b.reloads = b.reloads.wrapping_add(1);
}
