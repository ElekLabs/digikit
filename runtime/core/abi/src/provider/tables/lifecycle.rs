use crate::core_api::CoreApi;
use crate::provider::{code, Consumer, ConsumerId, Provider};
use crate::service::{ApplyCtx, SettleCtx};
use core::ffi::c_void;

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
        LifecycleV1 {
            attach: attach::<C, P>,
            detach: detach::<C, P>,
            can_stop: can_stop::<C, P>,
            settle: settle::<C, P>,
            reload_done: reload_done::<C, P>,
        }
    }
}

unsafe extern "C" fn attach<C: CoreApi, P: Provider<C>>(
    inst: *mut c_void,
    core: *const c_void,
    c: *const ApplyCtx,
) -> i32 {
    let c = &*c;
    let consumer = Consumer::of_section(ConsumerId::new(c.owner), c.reg, c.base);
    code((*(inst as *mut P)).attach(&*(core as *const C), &consumer))
}

unsafe extern "C" fn detach<C: CoreApi, P: Provider<C>>(
    inst: *mut c_void,
    core: *const c_void,
    owner: u8,
) {
    (*(inst as *mut P)).detach(&*(core as *const C), ConsumerId::new(owner))
}

unsafe extern "C" fn can_stop<C: CoreApi, P: Provider<C>>(
    inst: *mut c_void,
    core: *const c_void,
) -> i32 {
    code((*(inst as *mut P)).can_stop(&*(core as *const C)))
}

unsafe extern "C" fn settle<C: CoreApi, P: Provider<C>>(
    inst: *mut c_void,
    core: *const c_void,
    s: *const SettleCtx,
) {
    (*(inst as *mut P)).settle(&*(core as *const C), &*s)
}

unsafe extern "C" fn reload_done<C: CoreApi, P: Provider<C>>(
    inst: *mut c_void,
    core: *const c_void,
) {
    (*(inst as *mut P)).reload_done(&*(core as *const C))
}
