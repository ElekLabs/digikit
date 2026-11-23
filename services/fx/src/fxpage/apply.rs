//! Apply and remove the pages of a module: rows, parameters and list entries.

use super::*;
use crate::dsp as fxdsp;
use crate::fxsend;
use core::ptr::addr_of_mut;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::mem::{iw, rd32};
use dkrt_sdk::*;

/// Fill the page, the parameter rows and the list for the EFFECT record at `b` of module `owner` (module record + 1). The effect's row in the chain has been
/// claimed by `dsp::reserve`; the page and parameter rows belong to that row's number.
pub unsafe fn apply<C: CoreApi>(core: &C, owner: u8, b: *const u8) -> i32 {
    let inf = info(core);
    if inf.is_null() {
        return E_NO_REGISTRY;
    }
    let s = fs();
    let Some(k) = fxdsp::fx_row_of(core, owner, rd32(b)) else {
        return E_FORMAT; // no such effect in the module's DSP section
    };
    if k >= iw(inf, w::PAGES) as usize
        || k >= MAX_FX
        || (iw(inf, w::VALUES_N) as usize) < (k + 1) * WORDS
    {
        return E_PAGE_FULL;
    }
    if s.owner[k] != 0 {
        return E_PAGE_FULL;
    }
    let n = rd32(b.add(8)) as usize;
    let first = iw(inf, w::P_FIRST) + (k * WORDS) as u32;
    let row = iw(inf, w::ROWS) + k as u32 * iw(inf, w::ROW_SIZE);
    // strings
    copy_text(s.tab[k].as_mut_ptr(), b.add(16), 8);
    copy_text(s.title[k].as_mut_ptr(), b.add(24), TEXT);
    let tab = dkrt_svc_params::make_rep(s.rep_tab[k].as_mut_ptr(), s.tab[k].as_ptr(), false);
    let title = dkrt_svc_params::make_rep(s.rep_title[k].as_mut_ptr(), s.title[k].as_ptr(), false);
    // the page's nine codes: 0 where no knob stands
    let mut codes = [0u32; 9];
    let mut j = 0;
    while j < WORDS {
        let wi = k * WORDS + j;
        let (code, tpl, def_n) = if j < n {
            let kr = b.add(KNOB_AT + j * KNOB_REC);
            let (lo, hi) = (
                f32::from_bits(rd32(kr.add(8))),
                f32::from_bits(rd32(kr.add(12))),
            );
            copy_text(s.name[wi].as_mut_ptr(), kr.add(20), TEXT);
            copy_text(s.short[wi].as_mut_ptr(), kr.add(20 + TEXT), 8);
            let pos = rd32(kr) as usize;
            let tpl = if rd32(kr.add(4)) == 1 {
                iw(inf, w::TPL_BIPOLAR)
            } else {
                iw(inf, w::TPL_PERCENT)
            };
            let d = raw_of(f32::from_bits(rd32(kr.add(16))), lo, hi);
            s.min[k][j] = lo.to_bits();
            s.span[k][j] = (hi - lo).to_bits();
            codes[pos] = first + j as u32;
            (first + j as u32, tpl, d)
        } else if j == WORDS - 1 {
            copy_text(s.name[wi].as_mut_ptr(), b"Mix\0".as_ptr(), 4);
            copy_text(s.short[wi].as_mut_ptr(), b"MIX\0".as_ptr(), 4);
            codes[8] = first + j as u32;
            (
                first + j as u32,
                iw(inf, w::TPL_MIX),
                rd32(b.add(4)) as i32 * 256,
            )
        } else {
            j += 1;
            continue; // a word without a knob
        };
        let idx = value_index(inf, k, j);
        write_knob_row(core, s, k, j, code, tpl, idx, def_n);
        j += 1;
    }
    s.nknob[k] = n as u8;
    let id = rd32(b.add(12));
    publish_row(core, inf, k, id);
    let live = kit(core, inf);
    // the effect that left this row comes back (a reload or a live load after an unload), and the live kit is the one it left, with no kit loaded or initialised since:
    // the row's values in the array are the user's, so they stay and the next tick sends them
    let back = live != 0
        && id != 0
        && s.parked_id[k] == id
        && s.parked_mod[k] == fxsend::name_hash(core.module_name(owner))
        && s.parked_kit[k] == live
        && s.parked_hooks[k] == hook_runs()
        && core.rd16(live + iw(inf, w::ARRAY_OFF) + 2 * iw(inf, w::MARK_INDEX))
            == iw(inf, w::MARK_VALUE) as i32;
    s.parked_id[k] = 0;
    s.parked_mod[k] = 0;
    if live != 0 && !back {
        // a kit object is live already and holds nothing of this effect (the init hook ran before the effect was here): its defaults now
        seed_row(core, inf, k, live + iw(inf, w::ARRAY_OFF));
        mark(core, inf, live + iw(inf, w::ARRAY_OFF));
        s.seeds += 1;
    }
    fxdsp::fx_set_mix(core, k, mix_value(rd32(b.add(4)) as i32 * 256));
    // the page row: codes, then the strings; the owner and the list publish it
    let mut c = 0;
    while c < 9 {
        core.wr(row + 8 + 4 * c as u32, codes[c]);
        c += 1;
    }
    core.wr(row, tab);
    core.wr(row + 4, title);
    s.owner[k] = owner;
    rebuild_list(core, inf);
    fxsend::apply(core, k, owner, s.tab[k].as_ptr());
    OK
}

/// Give back the pages of module `owner`: out of the list first, then the rows back to the reserved state.
pub unsafe fn remove<C: CoreApi>(core: &C, owner: u8) {
    let inf = info(core);
    if inf.is_null() {
        return;
    }
    let s = fs();
    let mut gone = 0u32;
    let mut k = 0;
    while k < MAX_FX {
        if s.owner[k] == owner {
            s.owner[k] = 0;
            s.parked_id[k] = s.id[k];
            s.parked_mod[k] = fxsend::module_hash(k);
            s.parked_kit[k] = kit(core, inf);
            s.parked_hooks[k] = hook_runs();
            gone |= 1 << k;
        }
        k += 1;
    }
    if gone == 0 {
        return;
    }
    rebuild_list(core, inf);
    k = 0;
    while k < MAX_FX {
        if gone & (1 << k) != 0 {
            let row = iw(inf, w::ROWS) + k as u32 * iw(inf, w::ROW_SIZE);
            let mut c = 0;
            while c < 9 {
                core.wr(row + 8 + 4 * c as u32, 0);
                c += 1;
            }
            // name and title: the empty string, so no word of the row points to the state of the effect that left
            let empty = dkrt_svc_params::make_rep(
                addr_of_mut!(DKRT_FXP_EMPTY) as *mut u8,
                b"\0".as_ptr(),
                false,
            );
            core.wr(row, empty);
            core.wr(row + 4, empty);
            // the defaults go to 0 before the row's table is published: the kit hooks must not keep the defaults of the effect that left
            let mut j = 0;
            while j < WORDS {
                s.def[k][j] = 0;
                j += 1;
            }
            publish_row(core, inf, k, 0);
            j = 0;
            while j < WORDS {
                dkrt_svc_params::reset_param_row(
                    core,
                    dkrt_svc_params::dynreg(core),
                    iw(inf, w::P_FIRST) + (k * WORDS + j) as u32,
                );
                s.cur[k][j] = 0;
                s.def[k][j] = 0;
                s.dsp[k][j] = -1;
                s.want[k][j] = -1;
                j += 1;
            }
            s.nknob[k] = 0;
            fxdsp::fx_forget(core, k);
            fxsend::remove(core, k);
        }
        k += 1;
    }
}

/// Hand the kit hooks of `core/fxpages` the effect ID of row `k` (0: none) and its defaults. An effect's defaults go first and its ID after them, so a kit
/// load between the two stores cannot restore values against defaults that are not there yet; a row's ID goes to 0 first for the same reason.
unsafe fn publish_row<C: CoreApi>(core: &C, inf: *const u32, k: usize, id: u32) {
    let s = fs();
    let (ids, defs) = (iw(inf, w::IDS), iw(inf, w::DEFS));
    if id == 0 {
        core.wr_half(ids + 2 * k as u32, 0);
    }
    let mut j = 0;
    while j < WORDS {
        core.wr_half(defs + 2 * (k * WORDS + j) as u32, s.def[k][j] as u32);
        j += 1;
    }
    if id != 0 {
        core.wr_half(ids + 2 * k as u32, id);
    }
    s.id[k] = id;
}
