//! A generated core loaded from a library built from `packages/emulator/sharc-core-lib`.
//!
//! The library exports `sharc_core_header` and `sharc_core`. [`load`] reads the header first and
//! refuses the library, naming the differing field, unless it equals the host's own
//! (`sharc_rt::header::HOST`: header version, `sharc-rt` source hash, rustc, target, layouts of
//! `St`, `Mem`, `Insn`, `V`, `Cfg` and `GenCore`). Only then does it hand out the core. A library is
//! never unloaded, so the core is `'static`.
//!
//! Host and library must use the system allocator (no `#[global_allocator]` exists in the workspace:
//! a `Vec` or `Box` made on one side is freed on the other) and the `unwind` panic strategy; the header's
//! `flags` field carries the panic strategy, the optimisation level, the profile and a hash of the
//! rustflags (PGO-only flags excluded: a PGO host and a plain library accept each other), and a library built
//! with other flags is refused.

#![cfg(any(unix, windows))]

use crate::GenCore;
use sharc_rt::header::{CoreHeader, HOST};
use std::collections::HashMap;
use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

type HeaderFn = unsafe extern "C" fn() -> *const CoreHeader;
type CoreFn = unsafe extern "C" fn() -> *const GenCore;

type Loaded = Mutex<Option<HashMap<(PathBuf, &'static str), &'static GenCore>>>;

static LOADED: Loaded = Mutex::new(None);

/// The core of the library at `path`, or why the library is refused. `env_suffix` is the host's
/// (`GenCore::env_suffix`): the table is copied once per library and suffix, with that suffix.
pub fn load(path: &Path, env_suffix: &'static str) -> Result<&'static GenCore, String> {
    let key = (path.to_path_buf(), env_suffix);
    if let Some(core) = LOADED.lock().unwrap().get_or_insert_default().get(&key) {
        return Ok(core);
    }
    let lib = dynlib::open(path)?;
    let sym = |name: &str| {
        let n = CString::new(name).expect("symbol name");
        let p = dynlib::symbol(lib, &n);
        if p.is_null() {
            Err(format!("{}: no symbol {name}", path.display()))
        } else {
            Ok(p)
        }
    };
    // SAFETY: the symbols are the C ABI of sharc-core-lib; the header is read as its repr(C) type only
    // after its first field (the header version) equals the host's, and the core only after every
    // field matched.
    let core = unsafe {
        let header =
            std::mem::transmute::<*mut std::ffi::c_void, HeaderFn>(sym("sharc_core_header")?);
        let core = std::mem::transmute::<*mut std::ffi::c_void, CoreFn>(sym("sharc_core")?);
        let p = header();
        if p.is_null() {
            return Err(format!("{}: no header", path.display()));
        }
        let version = *(p as *const u32);
        if version != HOST.abi_version {
            return Err(format!(
                "{}: core library refused: header version: library {version}, host {}",
                path.display(),
                HOST.abi_version
            ));
        }
        if let Some(diff) = (*p).first_difference(&HOST) {
            return Err(format!("{}: core library refused: {diff}", path.display()));
        }
        &*core()
    };
    let core: &'static GenCore = Box::leak(Box::new(GenCore {
        env_suffix,
        ..*core
    }));
    // The library has its own instruction table cache: parse it now, not on the first audio block.
    if core.has_image {
        (core.insn_at)(0);
    }
    LOADED
        .lock()
        .unwrap()
        .get_or_insert_default()
        .insert(key, core);
    Ok(core)
}
