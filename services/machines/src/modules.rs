//! The machines of one module record: the MODULES reply (`module_info`) and the machine types its rows hold (`types_of`).
use crate::registry::{rstate, MAX_MACH};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::service::ModuleMachines;

/// What the MODULES reply says of the machines of module record `owner` (1 ..): the voice rows it owns (mask), its machine types (the machine rows of the registry, then
/// the types of its voice rows that no machine row names: a DSP-only module, at most 8) and the guard's counts for them (the export `dk_machines_module_info`).
pub fn module_info<C: CoreApi>(core: &C, owner: u8) -> ModuleMachines {
    let mut types = [0u8; MAX_MACH];
    let mut nt = types_of(core, owner, &mut types);
    let v = crate::rows::vrows();
    let mut r = 0;
    while r < dkrt_abi::dsp::MAX_ROWS {
        if v.row_owner[r] == owner && v.row_type[r] < 256 && nt < MAX_MACH {
            let t = v.row_type[r] as u8;
            let mut dup = false;
            let mut j = 0;
            while j < nt {
                dup |= types[j] == t;
                j += 1;
            }
            if !dup {
                types[nt] = t;
                nt += 1;
            }
        }
        r += 1;
    }
    let mut faults = 0u32;
    let mut off = false;
    let mut j = 0;
    while j < nt {
        faults = faults.wrapping_add(crate::guard::faults(types[j] as u32));
        off |= crate::guard::disabled(types[j] as u32);
        j += 1;
    }
    ModuleMachines {
        rows: if owner >= 1 {
            v.mod_rows[owner as usize - 1]
        } else {
            0
        },
        nt: nt as u32,
        types,
        faults,
        guard_off: off as u32,
    }
}

/// The machine type numbers of the machine rows `owner` filled (at most 8), in type order; returns how many.
pub fn types_of<C: CoreApi>(core: &C, owner: u8, out: &mut [u8; MAX_MACH]) -> usize {
    let s = rstate();
    // A device without a profile has no run-time types (as check_types refuses them): none, not the stock types from 0.
    let Some(first) = crate::profile(core).map(|p| p.dyn_type_first) else {
        return 0;
    };
    let mut n = 0;
    let mut k = 0;
    while k < MAX_MACH {
        if s.mach_owner[k] == owner && n < MAX_MACH {
            out[n] = (first + k as u32) as u8;
            n += 1;
        }
        k += 1;
    }
    n
}
