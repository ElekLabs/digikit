//! The voice rows the machines service owns: for each free voice row of the DSP's registry (`core/dyndsp`) the machine type claimed for it, its owner, whether its
//! type word was written to the DSP, and what the service publishes for it; for each module record the rows it owns. One `repr(C)` object, `DKRT_VROWS`, that a host
//! can read by its symbol; the DSP loader's own state (`DspState`) holds none of it. The rows start free (`FREE_TYPE`): the accessor sets that on first use.
use core::ptr::addr_of_mut;
use dkrt_abi::dsp::{FREE_TYPE, MAX_ROWS};
use dkrt_abi::REC_MAX;

/// `VROW`, the mark that the rows were initialised.
const MAGIC: u32 = 0x5652_4f57;

/// Per voice row: the type claimed for it (`FREE_TYPE` when free), the pc to publish, the declared ranges of knobs 0 to 7 (min | max << 16, 0: undeclared), its owner
/// (module record + 1, `PLACEHOLDER_OWNER` for a MISSING placeholder, 0: free), whether its type word was written to the DSP (so it needs clearing; 2: it reuses a
/// placeholder that the DSP holds), the note_on pc (0: none), the byte address of voice 0's state, the bytes between states, the bytes cleared at note-on, and the latch
/// mask. Per module record: the voice rows it owns (bit mask).
#[repr(C)]
pub struct VoiceRows {
    pub magic: u32,
    pub mod_rows: [u32; REC_MAX],
    pub row_type: [u32; MAX_ROWS],
    pub row_pc: [u32; MAX_ROWS],
    pub row_rng: [[u32; 8]; MAX_ROWS],
    pub row_owner: [u8; MAX_ROWS],
    pub row_pub: [u8; MAX_ROWS],
    pub row_note_on: [u32; MAX_ROWS],
    pub row_state_base: [u32; MAX_ROWS],
    pub row_stride: [u32; MAX_ROWS],
    pub row_clear: [u32; MAX_ROWS],
    pub row_latched: [u32; MAX_ROWS],
}

#[no_mangle]
pub static mut DKRT_VROWS: VoiceRows = unsafe { core::mem::zeroed() };

/// The voice rows; every row is free until a module claims it. A service runs on one task at a time (the UI loop and the job step do not overlap), and the MODULES
/// handler only reads.
pub fn vrows() -> &'static mut VoiceRows {
    let v = unsafe { &mut *addr_of_mut!(DKRT_VROWS) };
    if v.magic != MAGIC {
        let mut i = 0;
        while i < MAX_ROWS {
            v.row_type[i] = FREE_TYPE;
            i += 1;
        }
        v.magic = MAGIC;
    }
    v
}

/// The rows as bytes, for a host test that compares the state before and after a step.
pub fn bytes() -> &'static [u8] {
    let n = core::mem::size_of::<VoiceRows>();
    vrows();
    unsafe { core::slice::from_raw_parts(addr_of_mut!(DKRT_VROWS) as *const u8, n) }
}

/// Back to the start: every row free (the accessor sets that on the next use) and the guard's memory (`DKRT_GUARD`) has seen nothing. The host tests' reset between two
/// cores: every static of the service that holds DSP state.
pub fn reset() {
    unsafe { core::ptr::write_bytes(addr_of_mut!(DKRT_VROWS), 0, 1) }
    crate::modules::reset_published();
    crate::guard::reset();
}
