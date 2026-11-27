//! The machines of one module record: the MODULES reply (`module_info`) and the machine types its rows hold (`types_of`).
use crate::registry::{mach_owners, MAX_MACH};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::provider::WordSink;
use dkrt_abi::service::ModuleMachines;
use dkrt_abi::shared::Shared;
use dkrt_abi::REC_MAX;

// `describe` gives five words: the core refuses a section with more than `DESCRIBE_WORDS` words
const _: () = assert!(5 <= dkrt_abi::DESCRIBE_WORDS);

/// What `Describe` reads of one module record: its voice rows (mask), its number of machine types and the types. Copied in one masked section.
#[derive(Clone, Copy)]
struct View {
    rows: u32,
    nt: u8,
    types: [u8; MAX_MACH],
}

const NO_VIEW: View = View {
    rows: 0,
    nt: 0,
    types: [0; MAX_MACH],
};

/// The machine rows, the voice rows and the types they hold, for each module record, published as one table. `Describe` runs on the MODULES command handler's task and may
/// preempt the task that claims, parks or frees rows, so it never reads the owners of the machine rows, `DKRT_VROWS` and the profile one field after another (a claim in between
/// would give a mix of two states). Whoever changes these (the claim and release of voice rows, `clear_rows`, `set_mach_owner`, `clear_mach_owners`, the host tests that stand for
/// them) calls `publish` when its change is complete; `module_info` copies one entry in one section.
static VIEW: Shared<[View; REC_MAX]> = Shared::new([NO_VIEW; REC_MAX]);

/// What the voice rows and the machine rows say of module record `owner` (1 ..) now: read in the task that changes them, with no section held.
fn read_live<C: CoreApi>(core: &C, owner: u8) -> View {
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
    View {
        rows: v.mod_rows[owner as usize - 1],
        nt: nt as u8,
        types,
    }
}

/// Publish the machines of every module record for `Describe`: the state is read outside a section, then copied in one (`VIEW`). Call it when a change of the machine rows or
/// voice rows is complete.
pub fn publish<C: CoreApi>(core: &C) {
    let mut now = [NO_VIEW; REC_MAX];
    let mut o = 1;
    while o <= REC_MAX {
        now[o - 1] = read_live(core, o as u8);
        o += 1;
    }
    VIEW.with(core, |v| *v = now);
}

/// Forget what `publish` made known (the reset of a host test; the service starts with nothing published). No task runs meanwhile.
pub fn reset_published() {
    // SAFETY: a host test calls it between runs; no task runs meanwhile
    unsafe { VIEW.set_unmasked([NO_VIEW; REC_MAX]) };
}

/// What the MODULES reply says of the machines of module record `owner` (1 ..): the voice rows it owns (mask), its machine types (the machine rows of the registry, then
/// the types of its voice rows that no machine row names: a DSP-only module, at most 8) and the guard's counts for them (the export `dk_machines_module_info`). The rows and types
/// are one published entry (`publish`), copied in one section; the guard's counts are read after.
pub fn module_info<C: CoreApi>(core: &C, owner: u8) -> ModuleMachines {
    if owner == 0 || owner as usize > REC_MAX {
        return ModuleMachines::default();
    }
    let e = VIEW.with(core, |v| v[owner as usize - 1]);
    let (nt, types) = (e.nt as usize, e.types);
    let mut faults = 0u32;
    let mut off = false;
    let mut j = 0;
    while j < nt {
        faults = faults.wrapping_add(crate::guard::faults(types[j] as u32));
        off |= crate::guard::disabled(types[j] as u32);
        j += 1;
    }
    ModuleMachines {
        rows: e.rows,
        nt: nt as u32,
        types,
        faults,
        guard_off: off as u32,
    }
}

/// The words the MODULES reply carries for module record `owner` (1 ..), the `Describe` of the service. Five words: the voice rows it owns (mask); the guard's fault count for its
/// types; its number of machine types `nt` (bits 0 to 7) with bit 8 set when the guard switched one off; its types 0 to 3 and types 4 to 7 (one byte each, type `i` in byte `i % 4`
/// counted from the least significant). A module with nothing of this (no voice row, no type, no fault) gets no word, so the reply has no section for it. It runs on the
/// MODULES command handler's task, outside the owner: it reads only the `Shared` owners of the machine rows, the voice rows and the guard's words.
pub fn describe<C: CoreApi>(core: &C, owner: u8, out: &mut WordSink) {
    if owner == 0 || owner as usize > dkrt_abi::MAX_MODULES {
        return;
    }
    let m = module_info(core, owner);
    if m.rows == 0 && m.nt == 0 && m.faults == 0 && m.guard_off == 0 {
        return;
    }
    let pack = |lo: usize| {
        let mut w = 0u32;
        let mut i = 0;
        while i < 4 {
            w |= (m.types[lo + i] as u32) << (8 * i as u32);
            i += 1;
        }
        w
    };
    out.push(m.rows);
    out.push(m.faults);
    out.push(m.nt | (m.guard_off << 8));
    out.push(pack(0));
    out.push(pack(4));
}

/// The machine type numbers of the machine rows `owner` filled (at most 8), in type order; returns how many.
pub fn types_of<C: CoreApi>(core: &C, owner: u8, out: &mut [u8; MAX_MACH]) -> usize {
    // A device without a profile has no run-time types (as check_types refuses them): none, not the stock types from 0.
    let Some(first) = crate::profile(core).map(|p| p.dyn_type_first) else {
        return 0;
    };
    let owners = mach_owners(core);
    let mut n = 0;
    let mut k = 0;
    while k < MAX_MACH {
        if owners[k] == owner && n < MAX_MACH {
            out[n] = (first + k as u32) as u8;
            n += 1;
        }
        k += 1;
    }
    n
}
