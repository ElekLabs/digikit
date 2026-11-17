//! A generated core as a shared library. A host (`sharc_native::core_lib`) calls
//! `sharc_core_header`, compares it with its own header, and only then calls `sharc_core`.
//! The library holds its own copy of `sharc-rt`; see `sharc_rt::header`.

use sharc_rt::GenCore;
use sharc_rt::header::{CoreHeader, HOST};
use std::ffi::{CString, c_char};
use std::sync::OnceLock;

static HEADER: CoreHeader = HOST;
static HEADER_JSON: OnceLock<CString> = OnceLock::new();

/// A NUL-padded header string as JSON string content.
fn json_text(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end])
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            c => vec![c],
        })
        .collect()
}

fn header_json(h: &CoreHeader, image: &str) -> String {
    let layout: Vec<String> = h.layout.iter().map(u64::to_string).collect();
    format!(
        "{{\"abi_version\":{},\"sharc_rt_sha256\":\"{}\",\"rustc\":\"{}\",\"target\":\"{}\",\"flags\":\"{}\",\"layout\":[{}],\"image\":\"{}\"}}",
        h.abi_version,
        json_text(&h.sharc_rt_sha256),
        json_text(&h.rustc),
        json_text(&h.target),
        json_text(&h.flags),
        layout.join(","),
        json_text(image.as_bytes()),
    )
}

/// The same header and the image name of the core as one NUL-terminated JSON object (for tools that read a library without
/// the host's types: `verify_build cores` writes it to `header.json`). The string lives as long as the library.
#[unsafe(no_mangle)]
pub extern "C" fn sharc_core_header_json() -> *const c_char {
    HEADER_JSON
        .get_or_init(|| {
            CString::new(header_json(&HEADER, sharc_gen::CORE.image)).expect("header has no NUL")
        })
        .as_ptr()
}

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
