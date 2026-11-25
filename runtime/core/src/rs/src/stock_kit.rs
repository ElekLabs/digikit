//! Read-only stock kit values for the opt-in debug observer.
use crate::{
    dev,
    platform::{self, PLATFORM},
};
use core::ptr::{addr_of, read_volatile};
use dkrt_abi::{
    mem::{iw, rd16, rd32},
    platform::Platform,
    provider::name_hash,
};
use dkrt_sdk::{proto::E_AGAIN, E_NOT_FOUND, E_RANGE, OK};

// Published words of the FXPG v4 image adapter (image-modules/points/fxpages).
// The numbers in the table are generated from the device's [fx] profile.
const FXPAGES: u32 = name_hash("fxpages_info");
const FXPG: u32 = 0x4658_5047;
const KIT_PTR: usize = 19;
const SOUND_OFF: usize = 46;
const SOUND_STRIDE: usize = 47;
const TAIL: usize = 62;

pub(crate) unsafe extern "C" fn debug_read_parameter(
    scope: u32,
    track: u32,
    word: u32,
    out: *mut i32,
) -> i32 {
    let info = PLATFORM.info(FXPAGES);
    if info.is_null()
        || iw(info, 0) != FXPG
        || iw(info, 1) != 4
        || iw(info, 2) != platform::device()
    {
        return E_NOT_FOUND;
    }
    let kit_table = iw(info, KIT_PTR);
    let (pointer, offset) = if scope == 1 {
        let tracks = dev::tracks();
        let n = read_volatile(addr_of!(crate::dkrt_sound_words));
        if track >= tracks || word >= n {
            return E_RANGE;
        }
        (
            kit_table,
            iw(info, SOUND_OFF) + iw(info, SOUND_STRIDE) * track + crate::SOUND_WORDS_AT,
        )
    } else if scope == 2 && track == 0 && word < iw(info, TAIL) / 2 {
        // The live pointer table is [kit, FX values, track sounds...].
        // Read the FX view's actual array, not an inferred field of a kit copy.
        (kit_table + 4, 0)
    } else {
        return E_RANGE;
    };
    if out.is_null() {
        return OK;
    }
    let kit = rd32(pointer as *const u8);
    let Some(at) = kit
        .checked_add(offset)
        .and_then(|a| a.checked_add(2 * word))
    else {
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
