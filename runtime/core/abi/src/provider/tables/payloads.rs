use crate::core_api::CoreApi;
use crate::provider::{step_code, ConsumerId, Enter, Payloads, Plain, Provider};
use core::ffi::c_void;
use dkrt_sdk::{E_BUSY, OK};

/// The newest version of the payloads part the core knows.
pub const PAYLOADS_NEWEST: u16 = 1;

/// The payloads part, version 1 (tag `PART_PAYLOADS`): the bodies a module file carries for this service. `check` answers a status; `install` and `retire` answer a status or
/// [`Step::PENDING`](crate::provider::Step::PENDING) (`Step::of_code`). `body` is null with `has_body` false on every `install` call after the first of one payload.
#[repr(C)]
pub struct PayloadsV1 {
    pub check: unsafe extern "C" fn(*mut c_void, *const c_void, u32, *const u8, u32, u8) -> i32,
    pub install:
        unsafe extern "C" fn(*mut c_void, *const c_void, u32, *const u8, u32, bool, u8) -> i32,
    pub retire: unsafe extern "C" fn(*mut c_void, *const c_void, u8) -> i32,
}

// A table is immutable static data.
unsafe impl Sync for PayloadsV1 {}

impl PayloadsV1 {
    /// The table of owner object `P` over core `C`.
    pub const fn of<P: Provider<C> + Payloads<C>, C: CoreApi + 'static>() -> PayloadsV1 {
        Self::of_in::<P, C, Plain>()
    }

    /// As `of`, with the way a function reaches the instance: `G` (`Enter`; the service file's own `service!` gives the owner's guard).
    pub const fn of_in<P: Provider<C> + Payloads<C>, C: CoreApi + 'static, G: Enter<P>>(
    ) -> PayloadsV1 {
        PayloadsV1 {
            check: check::<C, P, G>,
            install: install::<C, P, G>,
            retire: retire::<C, P, G>,
        }
    }
}

unsafe fn body_of<'a>(body: *const u8, len: u32) -> &'a [u8] {
    if len == 0 {
        &[]
    } else {
        core::slice::from_raw_parts(body, len as usize)
    }
}

unsafe extern "C" fn check<C: CoreApi, P: Provider<C> + Payloads<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    kind: u32,
    body: *const u8,
    len: u32,
    consumer: u8,
) -> i32 {
    G::enter(inst, E_BUSY, |p: &mut P| {
        match p.check(
            &*(core as *const C),
            kind as u16,
            body_of(body, len),
            ConsumerId::new(consumer),
        ) {
            Ok(()) => OK,
            Err(status) => status,
        }
    })
}

unsafe extern "C" fn install<C: CoreApi, P: Provider<C> + Payloads<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    kind: u32,
    body: *const u8,
    len: u32,
    has_body: bool,
    consumer: u8,
) -> i32 {
    G::enter(inst, E_BUSY, |p: &mut P| {
        let body = has_body.then(|| body_of(body, len));
        step_code(p.install(
            &*(core as *const C),
            kind as u16,
            body,
            ConsumerId::new(consumer),
        ))
    })
}

unsafe extern "C" fn retire<C: CoreApi, P: Provider<C> + Payloads<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    who: u8,
) -> i32 {
    // a busy owner is not done: the caller asks again on a later pass
    G::enter(inst, crate::provider::Step::PENDING, |p: &mut P| {
        p.retire(&*(core as *const C), ConsumerId::new(who)).code()
    })
}
