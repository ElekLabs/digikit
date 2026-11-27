use crate::host::Host;
use core::ptr::addr_of_mut;
use dkrt_abi::part::{PartEnv, PartTable, TABLE_MAGIC};

/// The part's one host. The package calls its three entries in the UI task, one at a time.
static mut HOST: Host = Host::new();

fn host() -> &'static mut Host {
    unsafe { &mut *addr_of_mut!(HOST) }
}

unsafe extern "C" fn start(env: *const PartEnv) -> i32 {
    let info = (*env).info;
    host().start(&|hash| info(hash) as *mut u32)
}

unsafe extern "C" fn step(_: *const PartEnv) -> i32 {
    host().step()
}

unsafe extern "C" fn stop(_: *const PartEnv) -> i32 {
    host().stop()
}

/// The table the package finds through its directory (`patchkit syspkg build --part` writes its address into a slot).
#[no_mangle]
pub static DKPART_TABLE: PartTable = PartTable {
    magic: TABLE_MAGIC,
    name_hash: dkneeds::keys::name_hash_const("dsp-host"),
    start,
    step,
    stop,
};

#[cfg(target_arch = "m68k")]
extern "C" {
    /// The package's panic path (`dkpkg_part_panic`, `package/parts.rs`): `patchkit` resolves it when it links the part.
    fn dkpkg_part_panic() -> !;
}

#[cfg(target_arch = "m68k")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    unsafe { dkpkg_part_panic() }
}
