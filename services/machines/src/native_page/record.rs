//! The native page record (kinds `KIND` and `KIND_V2`): where each field sits in the body, `check` (the validation) and `apply` (the registration in the owner's `PageTable`).
//! The firmware's view, bitmap and event details are the service file's (`dks/src/native.rs`).
//! Body layout (the record without its kind and length words; every field a big endian word unless noted): `OFF_VERSION`, `OFF_TYPE` (the machine's type), `OFF_DRAW` and `OFF_EVENT` (offsets of
//! the callbacks in the module image), `OFF_TITLE` (`TITLE_LEN` bytes, NUL terminated), then v1: the bindings from `OFF_BINDINGS_V1`; v2: `OFF_PAGE` (0 or 1), `OFF_FLAGS`, the bindings from `OFF_BINDINGS_V2`.
//! A binding is `BINDING_BYTES` long: kind (0 absent, 1 PARAM, 2 LOCAL), value, encoder position.
use super::{PageTable, ENCODERS};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::mem::rd32;
use dkrt_sdk::*;

pub const KIND: u32 = records::KIND_NATIVE_PAGE;
pub const LEN: u32 = records::LEN_NATIVE_PAGE;
pub const KIND_V2: u32 = records::KIND_NATIVE_PAGE_V2;
pub const LEN_V2: u32 = records::LEN_NATIVE_PAGE_V2;

pub const OFF_VERSION: usize = 0;
pub const OFF_TYPE: usize = 4;
pub const OFF_DRAW: usize = 8;
pub const OFF_EVENT: usize = 12;
pub const OFF_TITLE: usize = 16;
pub const TITLE_LEN: usize = 24;
pub const OFF_BINDINGS_V1: usize = 40;
/// v2 only: the page index and the flags.
pub const OFF_PAGE: usize = 40;
pub const OFF_FLAGS: usize = 44;
pub const OFF_BINDINGS_V2: usize = 48;
pub const BINDING_BYTES: usize = 12;
pub const BINDINGS: usize = 8;

pub fn is_kind(kind: u32) -> bool {
    kind == KIND || kind == KIND_V2
}
pub fn binding_offset(kind: u32) -> usize {
    if kind == KIND_V2 {
        OFF_BINDINGS_V2
    } else {
        OFF_BINDINGS_V1
    }
}
pub unsafe fn page_index(kind: u32, body: *const u8) -> u32 {
    if kind == KIND_V2 {
        rd32(body.add(OFF_PAGE))
    } else {
        1
    }
}

/// Body excludes kind/length. PARAM indexes belong to this file; the caller also
/// checks that the referenced machine owns each PARAM through its ordinary rows.
pub unsafe fn check(kind: u32, body: *const u8, params: u32, image_size: u32) -> i32 {
    if !is_kind(kind) {
        return E_FORMAT;
    }
    let flags = if kind == KIND_V2 {
        rd32(body.add(OFF_FLAGS))
    } else {
        0
    };
    if flags & !3 != 0
        || (flags & 1 != 0 && page_index(kind, body) != 0)
        || (flags & 2 != 0 && page_index(kind, body) != 1)
    {
        return E_FORMAT;
    }
    let draw = rd32(body.add(OFF_DRAW));
    let event = rd32(body.add(OFF_EVENT));
    if rd32(body.add(OFF_VERSION)) != (if kind == KIND_V2 { 2 } else { 1 })
        || page_index(kind, body) > 1
        || draw >= image_size
        || event >= image_size
        || (draw | event) & 1 != 0
    {
        return E_FORMAT;
    }
    let mut terminated = false;
    let mut n = 0;
    while n < TITLE_LEN {
        let c = *body.add(OFF_TITLE + n);
        if c == 0 {
            terminated = n != 0;
            break;
        }
        if !(0x20..0x7f).contains(&c) {
            return E_FORMAT;
        }
        n += 1;
    }
    if !terminated {
        return E_FORMAT;
    }
    let mut positions = 0u32;
    let mut absent = false;
    let mut i = 0;
    while i < BINDINGS {
        let b = body.add(binding_offset(kind) + BINDING_BYTES * i);
        let (kind, value, pos) = (rd32(b), rd32(b.add(4)), rd32(b.add(8)));
        if kind == 0 {
            if value != 0 || pos != 0 {
                return E_FORMAT;
            }
            absent = true;
        } else {
            if absent
                || kind > 2
                || pos >= 8
                || positions & (1 << pos) != 0
                || (kind == 1 && value >= params)
                || (kind == 2 && value >= 256)
            {
                return E_FORMAT;
            }
            positions |= 1 << pos;
        }
        i += 1;
    }
    if positions == 0 {
        E_FORMAT
    } else {
        OK
    }
}

/// Registers the checked record in `pages`. `arg`: Some for a page (`dk_page_canvas`): its callbacks are `draw(arg, frame)` and `event(arg, event)`. The table refuses a page
/// twice, another type bound or a mode that mixes (`E_FORMAT`); `check` has refused every record that could come to that.
pub unsafe fn apply<C: CoreApi>(
    core: &C,
    pages: &mut PageTable,
    kind: u32,
    body: *const u8,
    base: u32,
    codes: &[u32],
    words: &[u32],
    arg: Option<u32>,
) -> i32 {
    if words.len() > BINDINGS {
        return E_FORMAT;
    }
    let mut bindings = [[0u32; 3]; ENCODERS];
    for (i, binding) in bindings.iter_mut().enumerate() {
        let b = body.add(binding_offset(kind) + BINDING_BYTES * i);
        binding[0] = rd32(b);
        let value = rd32(b.add(4));
        binding[1] = if binding[0] == 1 {
            codes[value as usize]
        } else {
            value
        };
        binding[2] = rd32(b.add(8));
    }
    let ty = crate::assign::resolve(core, rd32(body.add(OFF_TYPE))) as u32;
    let (mode, index, flags) = if kind == KIND_V2 {
        (2, page_index(kind, body), rd32(body.add(OFF_FLAGS)))
    } else {
        (1, 1, 0)
    };
    let title = core::slice::from_raw_parts(body.add(OFF_TITLE), TITLE_LEN);
    let (draw, event) = (
        base + rd32(body.add(OFF_DRAW)),
        base + rd32(body.add(OFF_EVENT)),
    );
    let result = pages.register(
        ty,
        index,
        mode,
        flags,
        title,
        draw,
        event,
        &bindings,
        words,
        arg.unwrap_or(0),
        arg.is_some(),
    );
    match result {
        Ok(()) => OK,
        Err(()) => E_FORMAT,
    }
}

#[cfg(test)]
mod tests;
