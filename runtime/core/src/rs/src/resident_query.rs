//! Optional reads from a loaded provider by resident image code on the UI task.

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use dkrt_sdk::{ResidentQueryV1, E_BUSY, E_CONTEXT, E_CONTRACT, E_MISSING, E_NOT_FOUND};

static mut UI_TASK: u32 = 0;
static mut ACTIVE: bool = false;

/// The UI loop is the only caller. Record its task once, before it dispatches panel events.
pub(crate) unsafe fn note_ui_task() {
    if read_volatile(addr_of!(UI_TASK)) == 0 {
        write_volatile(addr_of_mut!(UI_TASK), crate::io::current_task());
    }
}

/// A synchronous, read-only call from resident image code. The provider and caller must agree on the operation's layout.
/// A nonempty buffer has to point to the stated number of bytes for the duration of the call.
#[no_mangle]
pub unsafe extern "C" fn dkrt_service_query(
    provider: u32,
    minimum: u32,
    op: u32,
    request: *const u8,
    request_size: u32,
    reply: *mut u8,
    reply_size: u32,
) -> i32 {
    let ui = read_volatile(addr_of!(UI_TASK));
    if ui == 0 || crate::io::current_task() != ui {
        return E_CONTEXT;
    }
    if read_volatile(addr_of!(ACTIVE)) {
        return E_BUSY;
    }
    if matches!(
        crate::dlog::current_phase(),
        crate::dlog::PHASE_INIT | crate::dlog::PHASE_FINI
    ) {
        return E_CONTEXT;
    }
    if provider == 0
        || minimum == 0
        || (request_size != 0 && request.is_null())
        || (reply_size != 0 && reply.is_null())
    {
        return E_CONTRACT;
    }
    let addr = match crate::loader::resident_query(provider, minimum) {
        Ok(addr) => addr,
        Err(code) => return code,
    };
    // The UI task runs loader jobs and provider DSP failure teardown between events. The callback cannot wait,
    // pump the UI loop, or store its request/reply pointers. No loader reference is held across the call.
    let query: ResidentQueryV1 = core::mem::transmute(addr as usize);
    write_volatile(addr_of_mut!(ACTIVE), true);
    let result = query(op, request, request_size, reply, reply_size);
    write_volatile(addr_of_mut!(ACTIVE), false);
    // Only discovery reports absence of the facet. A callback can report a missing item.
    if result == E_MISSING {
        E_NOT_FOUND
    } else {
        result
    }
}
