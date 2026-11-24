//! The effect rows the fx service owns: for each free row of the effect chain (`core/dynfx`) the owner, whether its pc word was written to the DSP, and what
//! the service publishes for it; for each module record the rows it owns. One `repr(C)` object, `DKRT_FROWS`, zero at the start (a free row has owner 0), so a
//! host can read it by its symbol. The DSP loader's own state (`DspState`) holds none of it.
use core::ptr::addr_of_mut;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::REC_MAX;

/// Per effect row (the free rows of `core/dynfx`, in order): the owner (module record + 1, 0: free), whether its pc word was written to the DSP (so it needs clearing),
/// the pc, the byte addresses of the desc and the state, the words p0 to p7 to publish, the MIX word (`MIX_ONE` unless the effect's page says otherwise), the route and
/// the four amount words (published before the pc: a send effect never runs on the master), and the flags word of its record (`FXF_*`: the routes the effect may run
/// on and its default route; 0 = master only). Per module record: the effect rows it owns (bit mask).
#[repr(C)]
pub struct FxRows {
    pub fx_owner: [u8; MAX_FX],
    pub fx_pub: [u8; MAX_FX],
    pub fx_pc: [u32; MAX_FX],
    pub fx_desc: [u32; MAX_FX],
    pub fx_state: [u32; MAX_FX],
    pub fx_par: [[u32; 8]; MAX_FX],
    pub fx_mix: [u32; MAX_FX],
    pub fx_route: [u32; MAX_FX],
    pub fx_amt: [[u32; 4]; MAX_FX],
    pub fx_flags: [u32; MAX_FX],
    pub mod_fx: [u32; REC_MAX],
}

#[no_mangle]
pub static mut DKRT_FROWS: FxRows = unsafe { core::mem::zeroed() };

/// The effect rows. A service runs on one task at a time (the UI loop and the job step do not overlap), as the rows of the loader state were used before.
pub fn frows() -> &'static mut FxRows {
    unsafe { &mut *addr_of_mut!(DKRT_FROWS) }
}

/// The rows as bytes, for a host test that compares the state before and after a step.
pub fn bytes() -> &'static [u8] {
    unsafe {
        core::slice::from_raw_parts(
            addr_of_mut!(DKRT_FROWS) as *const u8,
            core::mem::size_of::<FxRows>(),
        )
    }
}

/// Back to the start: the rows are zero. The host tests' reset between two cores (the guard's memory is the owner's, so a new owner has seen nothing).
pub fn reset() {
    unsafe { core::ptr::write_bytes(addr_of_mut!(DKRT_FROWS), 0, 1) }
}
