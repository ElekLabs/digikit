//! A generated core as a shared library. A host (`sharc_native::core_lib`) calls
//! `sharc_core_header`, compares it with its own header, and only then calls `sharc_core`.
//! The library holds its own copy of `sharc-rt`; see `sharc_rt::header`.

use sharc_rt::GenCore;
use sharc_rt::header::{CoreHeader, HOST};

static HEADER: CoreHeader = HOST;

/// The header of this library: ABI version, sharc-rt source hash, rustc, target, type layouts.
#[unsafe(no_mangle)]
pub extern "C" fn sharc_core_header() -> *const CoreHeader {
    &HEADER
}

/// The generated core. Read it only when the header matches the host's.
#[unsafe(no_mangle)]
pub extern "C" fn sharc_core() -> *const GenCore {
    &sharc_gen::CORE
}
