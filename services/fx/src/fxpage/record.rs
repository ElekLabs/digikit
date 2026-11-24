//! Checks of an EFFECT record and the float and text conversions of its fields.

use super::*;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::mem::{cstr_len, rd32};
use dkrt_sdk::*;

unsafe fn text_ok(p: *const u8, field: usize, max: usize) -> bool {
    let n = cstr_len(p, field);
    if n == 0 || n > max || n == field {
        return false;
    }
    let mut i = 0;
    while i < field {
        let c = *p.add(i);
        if i < n {
            if !(0x20..0x7f).contains(&c) {
                return false;
            }
        } else if c != 0 {
            return false;
        }
        i += 1;
    }
    true
}

pub(super) unsafe fn copy_text(dst: *mut u8, src: *const u8, n: usize) {
    let mut i = 0;
    while i < n {
        *dst.add(i) = *src.add(i);
        i += 1;
    }
}

/// The fixed-size EFFECT record at `b` (its body), checked before anything is written; the registry's `fxpages` table must be there. The fields are the rule of `check_body`.
pub unsafe fn check_record<C: CoreApi>(core: &C, b: *const u8) -> i32 {
    let inf = info(core);
    if inf.is_null() {
        return E_NO_REGISTRY;
    }
    check_body(b)
}

/// The rule of one EFFECT record at `b` (its body, `LEN_EFFECT - 8` bytes): ranges, texts, knob positions, minimum below maximum with the default inside. Reads nothing but the
/// body (the page slots are checked by `apply`), so `Fx::validate_records` runs it on a built module.
pub unsafe fn check_body(b: *const u8) -> i32 {
    let (fx, mix, n) = (rd32(b), rd32(b.add(4)), rd32(b.add(8)) as usize);
    let id = rd32(b.add(12));
    if fx as usize >= MAX_FX || mix > 127 || n > KNOBS || id == 0 || id > 0xffff {
        return E_FORMAT;
    }
    if !text_ok(b.add(16), 8, 3) || !text_ok(b.add(24), TEXT, TEXT - 1) {
        return E_FORMAT;
    }
    let mut seen = 0u32;
    let mut j = 0;
    while j < KNOBS {
        let k = b.add(KNOB_AT + j * KNOB_REC);
        if j < n {
            let (pos, disp) = (rd32(k), rd32(k.add(4)));
            let (lo, hi, d) = (
                f32::from_bits(rd32(k.add(8))),
                f32::from_bits(rd32(k.add(12))),
                f32::from_bits(rd32(k.add(16))),
            );
            if pos >= 8 || seen & (1 << pos) != 0 || disp > 1 {
                return E_FORMAT;
            }
            seen |= 1 << pos;
            // finite, min < max, the default inside (the comparisons are false for a NaN)
            if !(lo < hi) || !(d >= lo && d <= hi) || (hi - lo) == f32::INFINITY {
                return E_FORMAT;
            }
            if !text_ok(k.add(20), TEXT, TEXT - 1) || !text_ok(k.add(20 + TEXT), 8, 6) {
                return E_FORMAT;
            }
        }
        j += 1;
    }
    OK
}

/// The raw value of a float default: the nearest of 0 .. 0x7f00 over [min, max].
pub(super) fn raw_of(def: f32, lo: f32, hi: f32) -> i32 {
    let f = (def - lo) / (hi - lo) * RAW_F + 0.5;
    let n = f as i32;
    if n < 0 {
        0
    } else if n > RAW_MAX {
        RAW_MAX
    } else {
        n
    }
}

/// The float a raw value stands for: min + span * (raw / 0x7f00); the maximum is exact.
#[inline]
pub(super) fn knob_value(min: u32, span: u32, raw: i32) -> u32 {
    (f32::from_bits(min) + f32::from_bits(span) * (raw as f32 / RAW_F)).to_bits()
}

/// MIX as a float: raw / 0x7f00 (0x7f00 is exactly 1.0, 0 exactly 0).
#[inline]
pub(super) fn mix_value(raw: i32) -> u32 {
    (raw as f32 / RAW_F).to_bits()
}
