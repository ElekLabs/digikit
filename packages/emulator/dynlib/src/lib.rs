//! The platform's dynamic loader, with no crate behind it: `dlopen`/`dlsym` from the C library on Unix,
//! `LoadLibraryW`/`GetProcAddress` on Windows. A library is opened once and never closed.
//! On other targets (wasm) the crate is empty.

#[cfg(unix)]
mod imp {
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::path::Path;

    unsafe extern "C" {
        fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
        fn dlerror() -> *mut c_char;
    }

    const RTLD_NOW: c_int = 2;

    fn last_error() -> String {
        // SAFETY: dlerror returns NULL or a NUL-terminated thread-local string.
        let p = unsafe { dlerror() };
        if p.is_null() {
            "unknown dlopen error".to_string()
        } else {
            // SAFETY: non-NULL, NUL-terminated (above).
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    }

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        let c = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| "library path contains NUL".to_string())?;
        // SAFETY: C is a valid NUL-terminated path.
        let lib = unsafe { dlopen(c.as_ptr(), RTLD_NOW) };
        if lib.is_null() {
            Err(format!("dlopen {}: {}", path.display(), last_error()))
        } else {
            Ok(lib)
        }
    }

    pub fn symbol(lib: *mut c_void, name: &CStr) -> *mut c_void {
        // SAFETY: LIB is a live dlopen handle, NAME NUL-terminated.
        unsafe { dlsym(lib, name.as_ptr()) }
    }
}

#[cfg(windows)]
mod imp {
    use std::ffi::{CStr, c_char, c_void};
    use std::os::windows::ffi::OsStrExt;
    use std::path::Path;

    unsafe extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        fn GetLastError() -> u32;
    }

    pub fn open(path: &Path) -> Result<*mut c_void, String> {
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: WIDE is a NUL-terminated UTF-16 path.
        let lib = unsafe { LoadLibraryW(wide.as_ptr()) };
        if lib.is_null() {
            // SAFETY: no other call ran in between.
            Err(format!(
                "LoadLibraryW {}: error {}",
                path.display(),
                unsafe { GetLastError() }
            ))
        } else {
            Ok(lib)
        }
    }

    pub fn symbol(lib: *mut c_void, name: &CStr) -> *mut c_void {
        // SAFETY: LIB is a live module handle, NAME NUL-terminated.
        unsafe { GetProcAddress(lib, name.as_ptr()) }
    }
}

#[cfg(any(unix, windows))]
pub use imp::{open, symbol};
