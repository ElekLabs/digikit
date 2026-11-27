use crate::core_api::CoreApi;
use crate::provider::{ConsumerId, Describe, Provider, WordSink};
use crate::provider::{Enter, Plain};
use crate::service::TableFault;
use core::ffi::c_void;

/// The newest version of the describe part the core knows.
pub const DESCRIBE_NEWEST: u16 = 1;

/// The describe part, version 1 (tag `PART_DESCRIBE`): capacity counts, the check of the service's tables and the words of a consumer for diagnostics.
#[repr(C)]
pub struct DescribeV1 {
    /// `out` points at two words: limit, used.
    pub cap: unsafe extern "C" fn(*mut c_void, *const c_void, u8, *mut u32) -> bool,
    pub table_check: unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut TableFault) -> u32,
    /// The core, the consumer's owner id, then `cap` words at `out`: the count of words written. It never enters the owner (`Describe::describe` has no object): it runs in any task.
    pub describe: unsafe extern "C" fn(*const c_void, u8, *mut u32, u32) -> u32,
}

// A table is immutable static data.
unsafe impl Sync for DescribeV1 {}

impl DescribeV1 {
    /// The table of owner object `P` over core `C`.
    pub const fn of<P: Provider<C> + Describe<C>, C: CoreApi + 'static>() -> DescribeV1 {
        Self::of_in::<P, C, Plain>()
    }

    /// As `of`, with the way a function reaches the instance: `G` (`Enter`; the service file's own `service!` gives the owner's guard).
    pub const fn of_in<P: Provider<C> + Describe<C>, C: CoreApi + 'static, G: Enter<P>>(
    ) -> DescribeV1 {
        DescribeV1 {
            cap: cap::<C, P, G>,
            table_check: table_check::<C, P, G>,
            describe: describe::<C, P, G>,
        }
    }
}

unsafe extern "C" fn cap<C: CoreApi, P: Provider<C> + Describe<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    kind: u8,
    out: *mut u32,
) -> bool {
    match G::enter(inst, None, |p: &mut P| p.cap(&*(core as *const C), kind)) {
        Some((l, u)) => {
            *out = l;
            *out.add(1) = u;
            true
        }
        None => false,
    }
}

unsafe extern "C" fn table_check<C: CoreApi, P: Provider<C> + Describe<C>, G: Enter<P>>(
    inst: *mut c_void,
    core: *const c_void,
    used: u32,
    out: *mut TableFault,
) -> u32 {
    G::enter(inst, 0, |p: &mut P| {
        p.table_check(&*(core as *const C), used, &mut *out)
    })
}

unsafe extern "C" fn describe<C: CoreApi, P: Provider<C> + Describe<C>, G: Enter<P>>(
    core: *const c_void,
    who: u8,
    out: *mut u32,
    cap: u32,
) -> u32 {
    let words = if cap == 0 {
        &mut [][..]
    } else {
        core::slice::from_raw_parts_mut(out, cap as usize)
    };
    let mut sink = WordSink::new(words);
    P::describe(&*(core as *const C), ConsumerId::new(who), &mut sink);
    sink.words().len() as u32
}
