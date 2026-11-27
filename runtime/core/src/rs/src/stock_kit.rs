//! Read-only stock kit values for the opt-in debug observer.
use crate::dev;
use core::ptr::{addr_of, read_volatile};
use dkrt_abi::mem::{rd, rd16};
use dkrt_sdk::{proto::E_AGAIN, E_NOT_FOUND, E_RANGE, OK};

/// Runtime-private data emitted by the device object from the device profile.
/// Reading stock Sound and kit values must not depend on a loaded service.
#[repr(C)]
#[derive(Clone, Copy)]
struct Layout {
    kit_table: u32,
    sound_off: u32,
    sound_stride: u32,
    kit_values_bytes: u32,
}

extern "C" {
    static dkrt_stock_kit_layout: Layout;
}

impl Layout {
    fn locate(
        self,
        scope: u32,
        track: u32,
        word: u32,
        tracks: u32,
        sound_words: u32,
    ) -> Result<(u32, u32), i32> {
        if self.kit_table == 0 {
            return Err(E_NOT_FOUND);
        }
        let (pointer, offset) = match scope {
            1 if track < tracks && word < sound_words => (
                self.kit_table,
                self.sound_off + self.sound_stride * track + crate::SOUND_WORDS_AT,
            ),
            // The second pointer is the live kit values. Do not infer it from a kit copy.
            2 if track == 0 && word < self.kit_values_bytes / 2 => (self.kit_table + 4, 0),
            _ => return Err(E_RANGE),
        };
        Ok((pointer, offset + 2 * word))
    }
}

pub(crate) unsafe extern "C" fn debug_read_parameter(
    scope: u32,
    track: u32,
    word: u32,
    out: *mut i32,
) -> i32 {
    if !dev::valid() {
        return E_NOT_FOUND;
    }
    let layout = read_volatile(addr_of!(dkrt_stock_kit_layout));
    let sound_words = read_volatile(addr_of!(crate::dkrt_sound_words));
    let (pointer, offset) = match layout.locate(scope, track, word, dev::tracks(), sound_words) {
        Ok(location) => location,
        Err(error) => return error,
    };
    if out.is_null() {
        return OK;
    }
    let kit = rd(pointer);
    let Some(at) = kit.checked_add(offset) else {
        return E_AGAIN;
    };
    if !dev::ddr_contains(kit)
        || !dev::ddr_contains(at)
        || !at.checked_add(1).is_some_and(dev::ddr_contains)
    {
        return E_AGAIN;
    }
    out.write(rd16(at));
    OK
}

#[cfg(test)]
mod tests;
