use crate::core_api::CoreApi;
use crate::provider::{code, Consumer, ConsumerId, Provider, Records};
use crate::provider::{Enter, Plain};
use crate::service::{ApplyCtx, CheckCtx, ModuleCommitCtx};
use core::ffi::c_void;
use dkrt_sdk::E_BUSY;

/// The newest version of the records part the core knows.
pub const RECORDS_NEWEST: u16 = 1;

/// The records part, version 1 (tag `PART_RECORDS`): the record kinds the service owns (kind, whole record length with its 8-byte header; the `kinds` argument of
/// `service!`) and the module-load callbacks. `validate` is host only and not in the table.
#[repr(C)]
pub struct RecordsV1 {
    pub kinds: *const (u32, u32),
    pub nkinds: u32,
    pub begin: unsafe extern "C" fn(*mut c_void, *const c_void, usize),
    pub commit:
        unsafe extern "C" fn(*mut c_void, *const c_void, usize, *const ModuleCommitCtx) -> i32,
    pub check: unsafe extern "C" fn(*mut c_void, *const c_void, u32, *const CheckCtx) -> i32,
    /// One record of an owned kind: its body (`len` bytes after the header) and the consumer's `ApplyCtx`.
    pub apply: unsafe extern "C" fn(
        *mut c_void,
        *const c_void,
        u32,
        *const u8,
        u32,
        *const ApplyCtx,
    ) -> i32,
}

// A table is immutable static data.
unsafe impl Sync for RecordsV1 {}

impl RecordsV1 {
    /// The table of owner object `P` over core `C` for the record kinds `kinds`.
    pub const fn of<P: Provider<C> + Records<C>, C: CoreApi + 'static>(
        kinds: &'static [(u32, u32)],
    ) -> RecordsV1 {
        Self::of_in::<P, C, Plain>(kinds)
    }

    /// As `of`, with the way a function reaches the instance: `G` (`Enter`; the service file's `service!` gives the owner's guard).
    pub const fn of_in<P: Provider<C> + Records<C>, C: CoreApi + 'static, G: Enter<P>>(
        kinds: &'static [(u32, u32)],
    ) -> RecordsV1 {
        RecordsV1 {
            kinds: kinds.as_ptr(),
            nkinds: kinds.len() as u32,
            begin: begin::<C, P, G>,
            commit: commit::<C, P, G>,
            check: check::<C, P, G>,
            apply: apply::<C, P, G>,
        }
    }

    /// The record kinds the service owns: (kind, record length).
    ///
    /// # Safety
    /// The table is valid (`ProviderHandle` checked it).
    pub unsafe fn kinds(&self) -> &'static [(u32, u32)] {
        core::slice::from_raw_parts(self.kinds, self.nkinds as usize)
    }
}

unsafe extern "C" fn begin<C: CoreApi, P: Provider<C> + Records<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    module: usize,
) {
    G::enter(inst, (), |p: &mut P| p.begin(&*(core as *const C), module))
}

unsafe extern "C" fn commit<C: CoreApi, P: Provider<C> + Records<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    module: usize,
    c: *const ModuleCommitCtx,
) -> i32 {
    G::enter(inst, E_BUSY, |p: &mut P| {
        code(p.commit(&*(core as *const C), module, &*c))
    })
}

unsafe extern "C" fn check<C: CoreApi, P: Provider<C> + Records<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    step: u32,
    c: *const CheckCtx,
) -> i32 {
    G::enter(inst, E_BUSY, |p: &mut P| {
        code(p.check(&*(core as *const C), step, &*c))
    })
}

unsafe extern "C" fn apply<C: CoreApi, P: Provider<C> + Records<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    kind: u32,
    body: *const u8,
    len: u32,
    c: *const ApplyCtx,
) -> i32 {
    let c = &*c;
    let consumer = Consumer::of_section(ConsumerId::new(c.owner), c.reg, c.base);
    let body = if len == 0 {
        &[][..]
    } else {
        core::slice::from_raw_parts(body, len as usize)
    };
    G::enter(inst, E_BUSY, |p: &mut P| {
        code(p.apply(&*(core as *const C), kind, body, &consumer))
    })
}
