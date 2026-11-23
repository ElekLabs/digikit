use crate::core_api::CoreApi;
use crate::dsp::{DspSec, Job};
use crate::provider::{code, DspPart, Provider};
use crate::service::{Claim, RowWrite};
use core::ffi::c_void;
use dkrt_sdk::fault::DIAG_WORDS;

/// The newest version of the DSP part the core knows.
pub const DSP_NEWEST: u16 = 1;

/// The DSP part, version 1 (tag `PART_DSP`): the eight steps of the DSP loader and the callbacks of its state machine. An `Option` result travels C-shaped: an out
/// pointer and a `bool`.
#[repr(C)]
pub struct DspV1 {
    pub check_sections:
        unsafe extern "C" fn(*mut c_void, *const c_void, *const DspSec, *mut Job) -> i32,
    pub check_rows: unsafe extern "C" fn(*mut c_void, *const c_void, *const DspSec) -> i32,
    pub check_types: unsafe extern "C" fn(*mut c_void, *const c_void, *const DspSec) -> i32,
    pub admit: unsafe extern "C" fn(*mut c_void, *const c_void, *const Job) -> i32,
    pub claim: unsafe extern "C" fn(*mut c_void, *const c_void, *mut Claim),
    pub release: unsafe extern "C" fn(*mut c_void, *const c_void, usize),
    pub clear: unsafe extern "C" fn(*mut c_void, *const c_void, usize) -> bool,
    pub row_step: unsafe extern "C" fn(
        *mut c_void,
        *const c_void,
        usize,
        *mut u32,
        bool,
        *mut RowWrite,
    ) -> bool,
    pub loaded: unsafe extern "C" fn(*mut c_void, *const c_void, usize),
    pub attribute: unsafe extern "C" fn(*mut c_void, *const c_void, u32, u32, *mut usize) -> bool,
    pub diag: unsafe extern "C" fn(*mut c_void, *const c_void, *const u32),
}

// A table is immutable static data.
unsafe impl Sync for DspV1 {}

impl DspV1 {
    /// The table of owner object `P` over core `C`.
    pub const fn of<P: Provider<C> + DspPart<C>, C: CoreApi + 'static>() -> DspV1 {
        DspV1 {
            check_sections: check_sections::<C, P>,
            check_rows: check_rows::<C, P>,
            check_types: check_types::<C, P>,
            admit: admit::<C, P>,
            claim: claim::<C, P>,
            release: release::<C, P>,
            clear: clear::<C, P>,
            row_step: row_step::<C, P>,
            loaded: loaded::<C, P>,
            attribute: attribute::<C, P>,
            diag: diag::<C, P>,
        }
    }
}

unsafe extern "C" fn check_sections<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    sec: *const DspSec,
    job: *mut Job,
) -> i32 {
    code((*(inst as *mut P)).check_sections(&*(core as *const C), &*sec, &mut *job))
}

unsafe extern "C" fn check_rows<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    sec: *const DspSec,
) -> i32 {
    code((*(inst as *mut P)).check_rows(&*(core as *const C), &*sec))
}

unsafe extern "C" fn check_types<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    sec: *const DspSec,
) -> i32 {
    code((*(inst as *mut P)).check_types(&*(core as *const C), &*sec))
}

unsafe extern "C" fn admit<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    job: *const Job,
) -> i32 {
    code((*(inst as *mut P)).admit(&*(core as *const C), &*job))
}

unsafe extern "C" fn claim<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    c: *mut Claim,
) {
    (*(inst as *mut P)).claim(&*(core as *const C), &mut *c)
}

unsafe extern "C" fn release<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    module: usize,
) {
    (*(inst as *mut P)).release(&*(core as *const C), module)
}

unsafe extern "C" fn clear<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    module: usize,
) -> bool {
    (*(inst as *mut P)).clear(&*(core as *const C), module)
}

unsafe extern "C" fn row_step<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    module: usize,
    want: *mut u32,
    write: bool,
    out: *mut RowWrite,
) -> bool {
    match (*(inst as *mut P)).row_step(&*(core as *const C), module, &mut *want, write) {
        Some(w) => {
            *out = w;
            true
        }
        None => false,
    }
}

unsafe extern "C" fn loaded<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    module: usize,
) {
    (*(inst as *mut P)).loaded(&*(core as *const C), module)
}

unsafe extern "C" fn attribute<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    site: u32,
    id: u32,
    out: *mut usize,
) -> bool {
    match (*(inst as *mut P)).attribute(&*(core as *const C), site, id) {
        Some(m) => {
            *out = m;
            true
        }
        None => false,
    }
}

/// `words` points at `DIAG_WORDS` words.
unsafe extern "C" fn diag<C: CoreApi, P: Provider<C> + DspPart<C>>(
    inst: *mut c_void,
    core: *const c_void,
    words: *const u32,
) {
    (*(inst as *mut P)).diag(&*(core as *const C), &*(words as *const [u32; DIAG_WORDS]))
}
