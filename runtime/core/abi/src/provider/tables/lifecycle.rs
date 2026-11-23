use crate::core_api::CoreApi;
use crate::provider::{code, Consumer, ConsumerId, Provider};
use crate::provider::{Enter, Plain};
use crate::service::{ApplyCtx, SettleCtx};
use core::ffi::c_void;
use dkrt_sdk::E_BUSY;

/// The newest version of the lifecycle part the core knows.
pub const LIFECYCLE_NEWEST: u16 = 1;

/// The lifecycle part, version 1 (tag `PART_LIFECYCLE`). The first argument of every function is the instance (the owner object), the second the core (`&C` of the
/// `C` the table was built for).
#[repr(C)]
pub struct LifecycleV1 {
    /// Every provider a module needs hears the consumer: `ApplyCtx` is its `register` section (null: no records) and the address its image is loaded at.
    pub attach: unsafe extern "C" fn(*mut c_void, *const c_void, *const ApplyCtx) -> i32,
    /// The consumer goes down; the argument is its owner id (module record + 1).
    pub detach: unsafe extern "C" fn(*mut c_void, *const c_void, u8),
    pub can_stop: unsafe extern "C" fn(*mut c_void, *const c_void) -> i32,
    pub settle: unsafe extern "C" fn(*mut c_void, *const c_void, *const SettleCtx),
    pub reload_done: unsafe extern "C" fn(*mut c_void, *const c_void),
}

// A table is immutable static data.
unsafe impl Sync for LifecycleV1 {}

impl LifecycleV1 {
    /// The table of owner object `P` over core `C`.
    pub const fn of<P: Provider<C>, C: CoreApi + 'static>() -> LifecycleV1 {
        Self::of_in::<P, C, Plain>()
    }

    /// As `of`, with the way a function reaches the instance: `G` (`Enter`; the service file's own `service!` gives the owner's guard).
    pub const fn of_in<P: Provider<C>, C: CoreApi + 'static, G: Enter<P>>() -> LifecycleV1 {
        LifecycleV1 {
            attach: attach::<C, P, G>,
            detach: detach::<C, P, G>,
            can_stop: can_stop::<C, P, G>,
            settle: settle::<C, P, G>,
            reload_done: reload_done::<C, P, G>,
        }
    }
}

unsafe extern "C" fn attach<C: CoreApi, P: Provider<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    c: *const ApplyCtx,
) -> i32 {
    let c = &*c;
    let consumer = Consumer::of_section(ConsumerId::new(c.owner), c.reg, c.base);
    G::enter(inst, E_BUSY, |p: &mut P| {
        code(p.attach(&*(core as *const C), &consumer))
    })
}

unsafe extern "C" fn detach<C: CoreApi, P: Provider<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    owner: u8,
) {
    G::enter(inst, (), |p: &mut P| {
        p.detach(&*(core as *const C), ConsumerId::new(owner))
    })
}

unsafe extern "C" fn can_stop<C: CoreApi, P: Provider<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
) -> i32 {
    G::enter(inst, E_BUSY, |p: &mut P| {
        code(p.can_stop(&*(core as *const C)))
    })
}

unsafe extern "C" fn settle<C: CoreApi, P: Provider<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    s: *const SettleCtx,
) {
    G::enter(inst, (), |p: &mut P| p.settle(&*(core as *const C), &*s))
}

unsafe extern "C" fn reload_done<C: CoreApi, P: Provider<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
) {
    G::enter(inst, (), |p: &mut P| p.reload_done(&*(core as *const C)))
}
