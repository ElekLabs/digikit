use crate::core_api::CoreApi;
use crate::provider::{Enter, Plain};
use crate::provider::{Provider, Ticker};
use core::ffi::c_void;

/// The newest version of the tick part the core knows.
pub const TICK_NEWEST: u16 = 1;

/// The tick part, version 1 (tag `PART_TICK`): work in one UI-loop pass, in a `service::tick_phase`.
#[repr(C)]
pub struct TickV1 {
    pub tick: unsafe extern "C" fn(*mut c_void, *const c_void, u32),
}

// A table is immutable static data.
unsafe impl Sync for TickV1 {}

impl TickV1 {
    /// The table of owner object `P` over core `C`.
    pub const fn of<P: Provider<C> + Ticker<C>, C: CoreApi + 'static>() -> TickV1 {
        Self::of_in::<P, C, Plain>()
    }

    /// As `of`, with the way a function reaches the instance: `G` (`Enter`; the service file's `service!` gives the owner's guard).
    pub const fn of_in<P: Provider<C> + Ticker<C>, C: CoreApi + 'static, G: Enter<P>>() -> TickV1 {
        TickV1 {
            tick: tick::<C, P, G>,
        }
    }
}

unsafe extern "C" fn tick<C: CoreApi, P: Provider<C> + Ticker<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    phase: u32,
) {
    G::enter(inst, (), |p: &mut P| p.tick(&*(core as *const C), phase))
}
