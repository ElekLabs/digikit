//! test/dyn-demo-stray: test/dyn-demo plus one store outside the module's own block, for the ColdFire memory audit
//! (`mise run module:audit`, tests/digikit/scenarios/memaudit-*.toml). Tick call number `STRAY_AT` writes one word `STRAY_OFFSET` bytes
//! above the module's counter: an address in the arena that this module does not own. The audit arms its guard after the boot, so the
//! call number lies in the audit's window. The audit must report exactly this one store.
#![no_std]

use core::panic::PanicInfo;

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {}
}

pub const MAGIC: u32 = 0x4459_4e4d; // "DYNM"
/// Distance of the stray store from the counter: far above the module's block, inside the 512 KiB arena.
pub const STRAY_OFFSET: usize = 0x2_0000;

/// The tick call that makes the stray store (once). The counter is at 15 when the audit arms and about 1200 at its end.
pub const STRAY_AT: u32 = 500;

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

/// [0] MAGIC after the first tick, [1] the number of ticks. Aligned to a cache line: the data shares no line with the code.
#[repr(align(16))]
pub struct Counter(pub [u32; 4]);

#[no_mangle]
pub static mut DYN_COUNTER: Counter = Counter([0; 4]);

unsafe extern "C" fn init(_ctx: *mut DkModuleCtx) -> i32 {
    let c = &raw mut DYN_COUNTER;
    c.write(Counter([0; 4]));
    0
}

unsafe extern "C" fn tick(_ctx: *mut DkModuleCtx) {
    let c = &raw mut DYN_COUNTER as *mut u32;
    let n = c.add(1).read_volatile();
    if n == STRAY_AT {
        // the stray store, once
        (c as *mut u8)
            .add(STRAY_OFFSET)
            .cast::<u32>()
            .write_volatile(0xdead_0001);
    }
    c.write_volatile(MAGIC);
    c.add(1).write_volatile(n.wrapping_add(1));
}

#[no_mangle]
#[allow(non_upper_case_globals)]
pub static dk_module: DkModuleDef = DkModuleDef {
    init: Some(init),
    fini: None,
    tick: Some(tick),
    menu: None,
};
