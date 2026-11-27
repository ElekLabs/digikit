//! The UI-loop pass: stored amounts into the Sound arrays and turns back to the settings.

use super::*;
use crate::dsp as fxdsp;
use crate::fxpage;
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::dsp;
use dkrt_abi::mem::iw;

pub(super) fn words_of(amt: &[u8; TRACKS]) -> [u32; 4] {
    let mut out = [0u32; 4];
    let mut t = 0;
    while t < TRACKS {
        out[t / 4] |= (amt[t] as u32) << (8 * (t % 4));
        t += 1;
    }
    out
}

unsafe fn put_amounts<C: CoreApi>(core: &C, k: usize) {
    let s = ss();
    let mut key = [0u8; KEY_MAX];
    let kn = key_of(&mut key, b"fx/send/", &s.id[k], s.idlen[k] as usize);
    core.settings_put(&key[..kn], &s.amt[k]);
    s.puts += 1;
}

/// Once per UI-loop pass (after the effect pages' own tick).
pub unsafe fn tick<C: CoreApi>(core: &C) {
    let s = ss();
    if s.used == 0 {
        return;
    }
    let inf = info(core);
    if inf.is_null() {
        return;
    }
    // a kit, a pattern's kit or a project was loaded: wait until the loads are over, then write the amounts back
    let ns = core.rd(iw(inf, w::NSOUND));
    if ns != s.nsound {
        s.nsound = ns;
        s.settle = SETTLE;
    }
    let mut quiet = true;
    if s.settle != 0 {
        s.settle -= 1;
        if s.settle == 0 {
            s.restore = 0xffff;
        } else {
            quiet = false;
        }
    }
    if quiet {
        tracks(core, inf);
    }
    // the amounts go to the link as live values; the DSP's own copy follows the link's confirmation
    let mut k = 0;
    while k < dsp::MAX_FX {
        if s.route[k] != 0 && fxdsp::fx_live(core, k) {
            let want = words_of(&s.amt[k]);
            let mut j = 0;
            while j < 4 {
                if let Some(slot) = fxdsp::fx_slot(core, k, dsp::FXW_AMT0 + j as u32) {
                    if want[j] != s.want[k][j] {
                        core.link_set(slot, want[j]);
                        s.want[k][j] = want[j];
                    }
                    if s.dsp[k][j] != s.want[k][j]
                        && core.link_confirmed(slot) == Some(s.want[k][j])
                    {
                        s.dsp[k][j] = s.want[k][j];
                        s.sent += 1;
                    }
                }
                j += 1;
            }
        }
        k += 1;
    }
    s.errors = core.link_errors();
}

/// The 16 Sound arrays: the stored amounts back into those that need them, a turn of the knob in the others.
pub(super) unsafe fn tracks<C: CoreApi>(core: &C, inf: *const u32) {
    let s = ss();
    let kit = fxpage::kit(core, inf);
    if kit == 0 {
        return;
    }
    if kit != s.kit {
        s.kit = kit;
        s.restore = 0xffff;
    }
    let (off, stride) = (iw(inf, w::SOUND_OFF), iw(inf, w::SOUND_STRIDE));
    let mut changed = 0u32; // effect rows whose amounts changed
    let mut t = 0;
    while t < TRACKS {
        let p = kit + off + stride * t as u32;
        let mut c = 0;
        while c < SEND_MAX {
            let k = s.row[c] as usize;
            if k != 0 {
                let k = k - 1;
                let at = p + SOUND_ARRAY + 2 * iw(inf, w::IDX0 + c);
                if s.restore & (1 << t) != 0 {
                    let raw = (s.amt[k][t] as i32) << 8;
                    core.wr16(at, raw);
                    s.shown[c][t] = raw;
                    s.zero_wait[c][t] = 0;
                } else {
                    let v = core.rd16(at);
                    if v == 0 && v != s.shown[c][t] && s.zero_wait[c][t] < ZERO_PASSES {
                        s.zero_wait[c][t] += 1;
                    } else if v != s.shown[c][t] {
                        s.zero_wait[c][t] = 0;
                        s.shown[c][t] = v;
                        let a = if v < 0 { 0 } else { (v >> 8).min(127) } as u8;
                        s.turns += 1;
                        if a != s.amt[k][t] {
                            s.amt[k][t] = a;
                            changed |= 1 << k;
                        }
                    } else {
                        s.zero_wait[c][t] = 0;
                    }
                }
            }
            c += 1;
        }
        if s.restore & (1 << t) != 0 {
            s.restore &= !(1 << t);
            s.restores += 1;
        }
        t += 1;
    }
    let mut k = 0;
    while k < dsp::MAX_FX {
        if changed & (1 << k) != 0 {
            put_amounts(core, k);
        }
        k += 1;
    }
}
