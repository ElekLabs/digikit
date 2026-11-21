//! The machine guard's diagnostics: the words 17 to 23 of the DSP's diagnostics block (`dkrt_sdk::fault`), logged as `MACHINE_FAULT` and `MACHINE_DISABLED`, and the
//! counts per machine type that the MODULES reply reports (`records::module_info`). The memory of what was seen is the object `DKRT_GUARD`; the host tests reset it with
//! `rows::reset`.
use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_abi::core_api::CoreApi;
use dkrt_sdk::fault::DIAG_WORDS;
use dkrt_sdk::logk;

/// Machine types the guard counts (the run-time types are below 32).
pub const GUARD_TYPES: usize = 32;

/// What `diag` has seen: the fault and disabled counts of the last block, the faults counted per machine type since the boot and the types switched off (bit `t`).
#[repr(C)]
pub struct Guard {
    pub faults_seen: u32,
    pub disabled_seen: u32,
    pub faults: [u32; GUARD_TYPES],
    pub off: u32,
}

#[no_mangle]
pub static mut DKRT_GUARD: Guard = Guard {
    faults_seen: 0,
    disabled_seen: 0,
    faults: [0; GUARD_TYPES],
    off: 0,
};

/// Forget everything `diag` has seen (host tests, between two cores).
pub fn reset() {
    unsafe {
        *addr_of_mut!(DKRT_GUARD) = Guard {
            faults_seen: 0,
            disabled_seen: 0,
            faults: [0; GUARD_TYPES],
            off: 0,
        }
    }
}

/// Faults the guard counted for machine type `t` since the boot, as the polls saw them (a volatile read: the MODULES handler runs on another task than the poll).
pub fn faults(t: u32) -> u32 {
    if (t as usize) < GUARD_TYPES {
        unsafe { read_volatile(addr_of!(DKRT_GUARD.faults[t as usize])) }
    } else {
        0
    }
}

/// Did the guard switch machine type `t` off?
pub fn disabled(t: u32) -> bool {
    t < GUARD_TYPES as u32 && unsafe { read_volatile(addr_of!(DKRT_GUARD.off)) } & (1 << t) != 0
}

/// The guard's words (17 to 23): a new fault is a `MACHINE_FAULT`, a newly disabled type a `MACHINE_DISABLED`. The poll is a second apart, so a burst is one event whose
/// fault count says how many (the last fault is the one named).
pub(crate) unsafe fn diag<C: CoreApi>(core: &C, buf: &[u32; DIAG_WORDS]) {
    let g = &mut *addr_of_mut!(DKRT_GUARD);
    let faults = buf[17] & 0xffff;
    let kind = (buf[17] >> 16) & 0xff;
    let disabled = buf[17] >> 24;
    if faults != g.faults_seen {
        // per type, for MODULES: the guard's words name only the type of the last fault, so a burst that spans two types within one poll counts for the last
        let t = (buf[18] & 0xffff) as usize;
        if t < GUARD_TYPES {
            let n = g.faults[t].wrapping_add(faults.wrapping_sub(g.faults_seen) & 0xffff);
            write_volatile(addr_of_mut!(g.faults[t]), n);
        }
        g.faults_seen = faults;
        core.log(
            logk::MACHINE_FAULT,
            logk::F_ERROR,
            [buf[18], kind | (faults << 8), buf[19], buf[20], buf[21]],
        );
    }
    if disabled != g.disabled_seen {
        let t = buf[23] as usize;
        if t < GUARD_TYPES {
            write_volatile(addr_of_mut!(g.off), g.off | 1 << t);
        }
        g.disabled_seen = disabled;
        core.log(
            logk::MACHINE_DISABLED,
            logk::F_ERROR,
            [buf[23], disabled, faults, 0, 0],
        );
    }
}
