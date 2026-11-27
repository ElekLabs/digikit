use crate::core_api::CoreApi;
use crate::provider::{code, Provider, State};
use crate::provider::{Enter, Plain};
use core::ffi::c_void;
use dkrt_sdk::E_BUSY;

/// The newest version of the state part the core knows.
pub const STATE_NEWEST: u16 = 1;

/// The state part, version 1 (tag `PART_STATE`): the bytes a service hands to its next copy over a reload. `version` and `accepts_from` are data, so the runtime checks a
/// handoff without entering the service. `export_max` and `export` answer 0 when the owner is busy; `import` answers `E_BUSY`. `from` is null when no bytes are given
/// (`has_from` false), which is not the same as an empty slice.
#[repr(C)]
pub struct StateV1 {
    pub version: u16,
    pub accepts_from: u16,
    pub export_max: unsafe extern "C" fn(*mut c_void, *const c_void) -> u32,
    pub export: unsafe extern "C" fn(*mut c_void, *const c_void, *mut u8, u32) -> u32,
    pub import: unsafe extern "C" fn(*mut c_void, *const c_void, *const u8, u32, bool) -> i32,
}

// A table is immutable static data.
unsafe impl Sync for StateV1 {}

impl StateV1 {
    /// The table of owner object `P` over core `C`.
    pub const fn of<P: Provider<C> + State<C>, C: CoreApi + 'static>() -> StateV1 {
        Self::of_in::<P, C, Plain>()
    }

    /// As `of`, with the way a function reaches the instance: `G` (`Enter`; the service file's own `service!` gives the owner's guard).
    pub const fn of_in<P: Provider<C> + State<C>, C: CoreApi + 'static, G: Enter<P>>() -> StateV1 {
        StateV1 {
            version: P::VERSION,
            accepts_from: P::ACCEPTS_FROM,
            export_max: export_max::<C, P, G>,
            export: export::<C, P, G>,
            import: import::<C, P, G>,
        }
    }
}

unsafe extern "C" fn export_max<C: CoreApi, P: Provider<C> + State<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
) -> u32 {
    G::enter(inst, 0, |p: &mut P| {
        p.export_max(&*(core as *const C)) as u32
    })
}

unsafe extern "C" fn export<C: CoreApi, P: Provider<C> + State<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    out: *mut u8,
    cap: u32,
) -> u32 {
    G::enter(inst, 0, |p: &mut P| {
        let out = core::slice::from_raw_parts_mut(out, cap as usize);
        p.export(&*(core as *const C), out) as u32
    })
}

unsafe extern "C" fn import<C: CoreApi, P: Provider<C> + State<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    from: *const u8,
    len: u32,
    has_from: bool,
) -> i32 {
    G::enter(inst, E_BUSY, |p: &mut P| {
        let from = has_from.then(|| core::slice::from_raw_parts(from, len as usize));
        code(p.import(&*(core as *const C), from))
    })
}
