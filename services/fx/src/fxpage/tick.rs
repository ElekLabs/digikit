//! The UI-loop pass: kit values against the DSP rows, and the knob words.

use super::*;
use crate::dsp as fxdsp;
use crate::fxsend;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::mem::iw;

/// Words of effect row `k` that exist: its knobs and MIX.
#[inline(never)]
pub(super) fn has_word(s: &FxpState, k: usize, j: usize) -> bool {
    j == WORDS - 1 || j < s.nknob[k] as usize
}

/// Once per UI-loop pass: note which values the DSP has taken, put the values in a new kit's array, and hand the changed values to the link; then the send effects' amounts.
pub unsafe fn tick<C: CoreApi>(core: &C) {
    let s = fs();
    let mut any = false;
    let mut k = 0;
    while k < MAX_FX {
        any |= s.owner[k] != 0;
        k += 1;
    }
    if !any {
        return;
    }
    let inf = info(core);
    if inf.is_null() {
        return;
    }
    confirm(core);
    values(core, inf);
    fxsend::tick(core);
}

/// The effect pages' values: the kit's array against the DSP rows.
pub(super) unsafe fn values<C: CoreApi>(core: &C, inf: *const u32) {
    let s = fs();
    let mut k;
    let kit = kit(core, inf);
    if kit == 0 {
        return;
    }
    let arr = kit + iw(inf, w::ARRAY_OFF);
    if core.rd16(arr + 2 * iw(inf, w::MARK_INDEX)) != iw(inf, w::MARK_VALUE) as i32 {
        // no mark: the hooks of the effect pages have not given this kit object its rows (a kit that was there before the effect was loaded: another pattern's, or one the
        // firmware cleared without them): the defaults, and the mark, before anything is read from it (a zero is never sent)
        k = 0;
        while k < MAX_FX {
            if s.owner[k] != 0 {
                seed_row(core, inf, k, arr);
            }
            k += 1;
        }
        mark(core, inf, arr);
        s.seeds += 1;
        s.kit = kit;
        return;
    }
    if kit != s.kit {
        s.adopts += 1; // a kit object with its rows: the live kit changed (a pattern change); the poll sends what differs from the DSP
        s.kit = kit;
        return;
    }
    s.polls = s.polls.wrapping_add(1);
    k = 0;
    while k < MAX_FX {
        if s.owner[k] != 0 {
            let mut j = 0;
            while j < WORDS {
                if has_word(s, k, j) {
                    let raw = core.rd16(arr + 2 * value_index(inf, k, j));
                    let n = if raw < 0 {
                        0
                    } else if raw > RAW_MAX {
                        RAW_MAX
                    } else {
                        raw
                    };
                    if n != s.cur[k][j] {
                        s.cur[k][j] = n;
                        s.changes += 1;
                    }
                    // only when the row is in the DSP and no load is going on; the link sends the newest value of the word when it is free
                    if n != s.want[k][j] && fxdsp::fx_live(core, k) {
                        set_knob(core, s, k, j, n);
                    }
                }
                j += 1;
            }
        }
        k += 1;
    }
}

/// The parameter row of word `j` of effect row `k` (the code is the row's), and the record's default as the value in force, in the kit and in the DSP row.
pub(super) unsafe fn write_knob_row<C: CoreApi>(
    c: &C,
    s: &mut FxpState,
    k: usize,
    j: usize,
    code: u32,
    tpl: u32,
    idx: u32,
    def_n: i32,
) {
    let wi = k * WORDS + j;
    dkrt_svc_rows::write_fx_param_row(
        c,
        dkrt_svc_rows::rows_info(c),
        code,
        tpl,
        idx,
        0,
        RAW_MAX as u32,
        def_n as u32,
        s.name[wi].as_ptr() as u32,
        s.short[wi].as_ptr() as u32,
    );
    s.cur[k][j] = def_n;
    s.def[k][j] = def_n;
    s.dsp[k][j] = def_n; // the DSP row gets the record's defaults (p words) and the mix below
    s.want[k][j] = def_n;
}

/// The link slot and the DSP word of word `j` of effect row `k` for the raw value `n`: knob j is word 4 + j (a float in [min, max]), MIX is word 13.
unsafe fn knob_word<C: CoreApi>(
    c: &C,
    s: &FxpState,
    k: usize,
    j: usize,
    n: i32,
) -> Option<(dkrt_abi::link::Slot, u32)> {
    let (word, bits) = if j == WORDS - 1 {
        (dkrt_abi::dsp::FXW_MIX, mix_value(n))
    } else {
        (4 + j as u32, knob_value(s.min[k][j], s.span[k][j], n))
    };
    fxdsp::fx_slot(c, k, word).map(|slot| (slot, bits))
}

/// Hand the raw value `n` of word `j` of effect row `k` to the link as the word's live value.
pub(super) unsafe fn set_knob<C: CoreApi>(
    c: &C,
    s: &mut FxpState,
    k: usize,
    j: usize,
    n: i32,
) -> bool {
    match knob_word(c, s, k, j, n) {
        Some((slot, bits)) => {
            c.link_set(slot, bits);
            s.want[k][j] = n;
            true
        }
        None => false,
    }
}

/// Note the values the DSP has taken: a word whose live value the link confirms is the DSP's.
unsafe fn confirm<C: CoreApi>(core: &C) {
    let s = fs();
    let mut k = 0;
    while k < MAX_FX {
        if s.owner[k] != 0 {
            let mut j = 0;
            while j < WORDS {
                let n = s.want[k][j];
                if has_word(s, k, j) && n >= 0 && n != s.dsp[k][j] {
                    if let Some((slot, bits)) = knob_word(core, s, k, j, n) {
                        if core.link_confirmed(slot) == Some(bits) {
                            s.dsp[k][j] = n;
                            s.sent += 1;
                        }
                    }
                }
                j += 1;
            }
        }
        k += 1;
    }
    s.errors = core.link_errors();
}

/// Pages in use and the pages the image reserved (for CAPACITY-like reports and tests): (reserved, used).
pub unsafe fn counts<C: CoreApi>(core: &C) -> (u32, u32) {
    let inf = info(core);
    if inf.is_null() {
        return (0, 0);
    }
    let s = fs();
    let mut used = 0;
    let mut k = 0;
    while k < MAX_FX {
        if s.owner[k] != 0 {
            used += 1;
        }
        k += 1;
    }
    (iw(inf, w::PAGES), used)
}
