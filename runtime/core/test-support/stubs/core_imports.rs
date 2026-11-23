//! The core imports (`dkc_*`) that the service file's code names, as host functions: this test binary links no runtime. Every one answers zero, except the two that tests
//! set: the device the file runs as and the module whose code runs.
use core::sync::atomic::{AtomicU32, Ordering::SeqCst};

pub static DEVICE: AtomicU32 = AtomicU32::new(0);
pub static RUNNING: AtomicU32 = AtomicU32::new(0);

fn answer<T: Copy>(name: &str, zero: T) -> T {
    let set = match name {
        "dkc_device_id" => DEVICE.load(SeqCst),
        "dkc_module_of" => RUNNING.load(SeqCst),
        _ => return zero,
    };
    // both imports return an integer of at most 32 bits, little or big endian: read it back through the integer
    assert!(core::mem::size_of::<T>() <= 4);
    let mut out = zero;
    // SAFETY: `T` is a plain integer of the size checked above; the low bytes of `set` are copied into it
    unsafe {
        let n = core::mem::size_of::<T>();
        let b = set.to_ne_bytes();
        let from = if cfg!(target_endian = "little") {
            0
        } else {
            4 - n
        };
        core::ptr::copy_nonoverlapping(b.as_ptr().add(from), &mut out as *mut T as *mut u8, n);
    }
    out
}

macro_rules! stub {
    () => {};
    (fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> !; $($rest:tt)*) => {
        #[no_mangle]
        extern "C" fn $name($(_: $ty),*) -> ! {
            panic!("the test reached {}", stringify!($name))
        }
        stub!($($rest)*);
    };
    (fn $name:ident($($arg:ident: $ty:ty),* $(,)?) -> $ret:ty; $($rest:tt)*) => {
        #[no_mangle]
        extern "C" fn $name($(_: $ty),*) -> $ret {
            // SAFETY: the imports return integers and pointers, zero is valid
            answer::<$ret>(stringify!($name), unsafe { core::mem::zeroed() })
        }
        stub!($($rest)*);
    };
    (fn $name:ident($($arg:ident: $ty:ty),* $(,)?); $($rest:tt)*) => {
        #[no_mangle]
        extern "C" fn $name($(_: $ty),*) {}
        stub!($($rest)*);
    };
}
dkrt_abi::core_imports!(stub);
