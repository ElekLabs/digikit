//! test/dyn-demo: the smallest module the digikit runtime can load. Packed into `dyn-demo.dkm` by `patchkit pack`.
//!
//! The entry point is the constant `dk_module` (docs/plans/sdk.md section 5), the same record as C's `const dk_module_def dk_module`. Its `tick` runs once per
//! pass of the UI task's event loop while the module is loaded: it stores a magic and adds one to a counter in the module's own bss (in the arena). `init`
//! zeroes the counter and logs its address (the log ring is in the runtime's state); `fini` leaves a marker behind. `dk_log` is an import of the runtime,
//! linked by name.
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

pub const MAGIC: u32 = 0x4459_4e4d; // "DYNM"
pub const FINI_MARK: u32 = 0xf1f1_0001;
const LOG_INFO: u32 = 2;

/// `dk_module_ctx`: opaque, the runtime passes it to every lifecycle function.
#[repr(C)]
pub struct DkModuleCtx {
    _private: [u8; 0],
}

/// `dk_module_def` of `<dk/core/module.h>`: the four lifecycle functions, each optional.
#[repr(C)]
pub struct DkModuleDef {
    pub init: Option<unsafe extern "C" fn(*mut DkModuleCtx) -> i32>,
    pub fini: Option<unsafe extern "C" fn(*mut DkModuleCtx)>,
    pub tick: Option<unsafe extern "C" fn(*mut DkModuleCtx)>,
    pub menu: Option<unsafe extern "C" fn(*mut DkModuleCtx, *mut u8)>,
}
unsafe impl Sync for DkModuleDef {}

extern "C" {
    fn dk_log(level: u32, msg: *const u8, value: u32);
}

/// [0] MAGIC after the first tick, [1] the number of ticks, [2] FINI_MARK after `fini`, [3] unused.
#[no_mangle]
pub static mut DYN_COUNTER: [u32; 4] = [0; 4];

unsafe extern "C" fn init(_ctx: *mut DkModuleCtx) -> i32 {
    let c = &raw mut DYN_COUNTER;
    c.write([0; 4]);
    dk_log(LOG_INFO, b"dyn-demo counter\0".as_ptr(), c as u32);
    0
}

unsafe extern "C" fn tick(_ctx: *mut DkModuleCtx) {
    let c = &raw mut DYN_COUNTER as *mut u32;
    c.write_volatile(MAGIC);
    c.add(1)
        .write_volatile(c.add(1).read_volatile().wrapping_add(1));
}

unsafe extern "C" fn fini(_ctx: *mut DkModuleCtx) {
    let c = &raw mut DYN_COUNTER as *mut u32;
    c.add(2).write_volatile(FINI_MARK);
    dk_log(
        LOG_INFO,
        b"dyn-demo fini\0".as_ptr(),
        c.add(1).read_volatile(),
    );
}

#[no_mangle]
#[allow(non_upper_case_globals)]
pub static dk_module: DkModuleDef = DkModuleDef {
    init: Some(init),
    fini: Some(fini),
    tick: Some(tick),
    menu: None,
};
