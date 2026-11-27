//! The WAV to native sample converter and the +Drive content hash. One copy for the card builder (`plusdrive-format` uses this crate) and for
//! the browser and the desktop webview (`apps/web/public/dksample.wasm`, built by `tools/nx/wasm.mjs`, C ABI below), so a sample
//! sent over USB is byte for byte the file the +Drive build writes (docs/MIDI-SYSEX-RPC.md section 12). No dependencies.
//!
//! `ds_convert(wav, len)` returns 0 and leaves the native file in the output buffer (`ds_out_ptr`/`ds_out_len`) and a JSON object in the info buffer
//! (`ds_info_ptr`/`ds_info_len`: `src_rate`, `channels`, `src_frames`, `frames`, `data_len`, `rpc_len`), or returns 1 and leaves `{"error": "..."}`
//! in the info buffer. The native file ends with the 16-byte trailer the firmware adds when it closes a file; a host that sends the file over
//! MidiRpc leaves the trailer off (`rpc_len`, the length without it). `ds_hash(data, len)` is the `hash | 1` of a file's record.
//! `ds_slot(wav, len, name, name_len, rate, max_bytes, big_endian)` makes the sample slot image of a device with numbered slots (slot.rs): 0 and the image in the
//! output buffer, the JSON in the info buffer (`src_rate`, `channels`, `src_frames`, `frames`, `data_len`, `name`), or 1 and `{"error": "..."}`.

pub mod hash;
pub mod native;
pub mod slot;
pub mod wav;

pub use hash::{content_hash, file_hash, hashlittle};
pub use native::{
    NATIVE_HEADER_SIZE, NATIVE_RATE, NATIVE_TRAILER_SIZE, NativeError, build_native_sample,
};
pub use slot::{
    SLOT_HEADER_SIZE, SLOT_NAME_MAX, SlotError, SlotFormat, SlotSample, slot_name, wav_to_slot,
};
pub use wav::{NativeInfo, NativeSample, WavError, WavLimits, wav_to_native};

use std::cell::RefCell;

#[derive(Default)]
struct State {
    out: Vec<u8>,
    info: Vec<u8>,
}

thread_local! {
    static ST: RefCell<State> = RefCell::new(State::default());
}

/// Convert `wav` to the native sample file and describe it as JSON. The error text is the converter's.
pub fn convert(wav: &[u8]) -> Result<(Vec<u8>, String), String> {
    let n = wav_to_native(wav, WavLimits::default()).map_err(|e| format!("{e:?}"))?;
    let info = format!(
        "{{\"src_rate\":{},\"channels\":{},\"src_frames\":{},\"frames\":{},\"data_len\":{},\"rpc_len\":{}}}",
        n.info.src_rate,
        n.info.channels,
        n.info.src_frames,
        n.info.frames,
        n.info.data_len,
        n.bytes.len().saturating_sub(NATIVE_TRAILER_SIZE)
    );
    Ok((n.bytes, info))
}

/// # Safety
/// `p..p+n` must be a readable range.
unsafe fn slice<'a>(p: *const u8, n: usize) -> &'a [u8] {
    if n == 0 {
        &[]
    } else {
        unsafe { std::slice::from_raw_parts(p, n) }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn ds_alloc(n: usize) -> *mut u8 {
    let mut v = Vec::<u8>::with_capacity(n.max(1));
    let p = v.as_mut_ptr();
    std::mem::forget(v);
    p
}

/// # Safety
/// `p` and `n` must be from `ds_alloc(n)`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ds_free(p: *mut u8, n: usize) {
    drop(unsafe { Vec::from_raw_parts(p, 0, n.max(1)) });
}

/// # Safety
/// `wav..wav+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ds_convert(wav: *const u8, len: usize) -> i32 {
    let r = convert(unsafe { slice(wav, len) });
    ST.with(|s| {
        let mut s = s.borrow_mut();
        match r {
            Ok((bytes, info)) => {
                s.out = bytes;
                s.info = info.into_bytes();
                0
            }
            Err(e) => {
                s.out = Vec::new();
                s.info = format!("{{\"error\":{:?}}}", e).into_bytes();
                1
            }
        }
    })
}

/// Convert `wav` to the slot image (`slot.rs`). `name` is the sample's name (UTF-8); the info JSON carries the name as written.
///
/// # Safety
/// `wav..wav+len` and `name..name+name_len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ds_slot(
    wav: *const u8,
    len: usize,
    name: *const u8,
    name_len: usize,
    rate: u32,
    max_bytes: usize,
    big_endian: u32,
) -> i32 {
    let name = String::from_utf8_lossy(unsafe { slice(name, name_len) }).into_owned();
    let fmt = SlotFormat {
        rate,
        max_bytes,
        big_endian: big_endian != 0,
    };
    let r = wav_to_slot(unsafe { slice(wav, len) }, &name, fmt);
    ST.with(|s| {
        let mut s = s.borrow_mut();
        match r {
            Ok(x) => {
                s.info = format!(
                    "{{\"src_rate\":{},\"channels\":{},\"src_frames\":{},\"frames\":{},\"data_len\":{},\"name\":{:?}}}",
                    x.src_rate, x.channels, x.src_frames, x.frames, x.data_len, x.name
                )
                .into_bytes();
                s.out = x.bytes;
                0
            }
            Err(e) => {
                s.out = Vec::new();
                s.info = format!("{{\"error\":{:?}}}", e.text()).into_bytes();
                1
            }
        }
    })
}

/// The `hash | 1` of a file's record for the file `data..data+len`.
///
/// # Safety
/// `data..data+len` must be readable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn ds_hash(data: *const u8, len: usize) -> u32 {
    content_hash(unsafe { slice(data, len) }) | 1
}

#[unsafe(no_mangle)]
pub extern "C" fn ds_out_ptr() -> *const u8 {
    ST.with(|s| s.borrow().out.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn ds_out_len() -> usize {
    ST.with(|s| s.borrow().out.len())
}
#[unsafe(no_mangle)]
pub extern "C" fn ds_info_ptr() -> *const u8 {
    ST.with(|s| s.borrow().info.as_ptr())
}
#[unsafe(no_mangle)]
pub extern "C" fn ds_info_len() -> usize {
    ST.with(|s| s.borrow().info.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(frames: usize, rate: u32) -> Vec<u8> {
        let pcm: Vec<u8> = (0..frames)
            .flat_map(|i| ((i * 37) as i16).to_le_bytes())
            .collect();
        let mut v = b"RIFF".to_vec();
        v.extend((36 + pcm.len() as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend([1, 0, 1, 0]);
        v.extend(rate.to_le_bytes());
        v.extend((rate * 2).to_le_bytes());
        v.extend([2, 0, 16, 0]);
        v.extend(b"data");
        v.extend((pcm.len() as u32).to_le_bytes());
        v.extend(pcm);
        v
    }

    #[test]
    fn a_48k_mono_wav_becomes_the_native_file() {
        let (bytes, info) = convert(&wav(1000, 48000)).unwrap();
        // header 0x40, 1000 frames of big-endian 16 bits, the 16-byte trailer
        assert_eq!(bytes.len(), 0x40 + 2000 + 16);
        assert_eq!(&bytes[0x40..0x42], &[0, 0]);
        assert_eq!(&bytes[0x42..0x44], &(37i16).to_be_bytes());
        assert!(info.contains("\"rpc_len\":2064"), "{info}");
    }

    #[test]
    fn the_c_abi_hash_is_the_record_hash() {
        let file = convert(&wav(10, 48000)).unwrap().0;
        assert_eq!(
            unsafe { ds_hash(file.as_ptr(), file.len()) },
            content_hash(&file) | 1
        );
        assert_eq!(unsafe { ds_hash(std::ptr::null(), 0) }, 0x43fa_243b);
    }

    #[test]
    fn the_c_abi_makes_a_slot_and_reports_a_refusal_as_json() {
        let w = wav(4, 48000);
        let name = b"kick";
        assert_eq!(
            unsafe {
                ds_slot(
                    w.as_ptr(),
                    w.len(),
                    name.as_ptr(),
                    name.len(),
                    48_000,
                    0x7ffc0,
                    1,
                )
            },
            0
        );
        assert_eq!(ds_out_len(), 0x40 + 8);
        let info = String::from_utf8(
            unsafe { std::slice::from_raw_parts(ds_info_ptr(), ds_info_len()) }.to_vec(),
        )
        .unwrap();
        assert!(
            info.contains("\"name\":\"kick\"") && info.contains("\"frames\":4"),
            "{info}"
        );
        // max_bytes 4 allows 2 samples: the 4-frame file is refused and the output is empty
        assert_eq!(
            unsafe { ds_slot(w.as_ptr(), w.len(), name.as_ptr(), name.len(), 48_000, 4, 1) },
            1
        );
        assert_eq!(ds_out_len(), 0);
    }

    #[test]
    fn the_c_abi_reports_an_error_as_json() {
        let bad = b"RIFFxxxxWAVE";
        assert_eq!(unsafe { ds_convert(bad.as_ptr(), bad.len()) }, 1);
        let n = ds_info_len();
        let s = unsafe { std::slice::from_raw_parts(ds_info_ptr(), n) };
        assert!(String::from_utf8_lossy(s).contains("error"));
        assert_eq!(ds_out_len(), 0);
    }
}
