//! The voice rows of the collected machines: their declared ranges and whether a module has one.
use super::*;
use crate::{assign, dsp};
use dkrt_abi::core_api::CoreApi;
use dkrt_abi::keys::KeysCopy;

/// The eight knob ranges and the latch mask of machine `m` for its voice row: knob position j holds `min | max << 16` of the param on it (0: none or out of range);
/// bit w of the mask is the machine's word w.
pub fn voice_declared(b: &Build, plan: &Plan, m: usize) -> ([u32; 8], u32) {
    let mut rng = [0u32; 8];
    let mut latched = 0u32;
    let mut i = 0;
    while i < b.nparam {
        if plan.file[i] != u8::MAX && plan.machine[i] as usize == m {
            let q = &b.params[i];
            let j = plan.pos[i] as usize;
            if q.min >= 0 && q.max <= 0xffff && q.min <= q.max && j < 8 {
                rng[j] = (q.min as u32) | (q.max as u32) << 16;
            }
            if q.latched {
                latched |= 1 << plan.word[i];
            }
        }
        i += 1;
    }
    (rng, latched)
}

/// Does module record `rec` have a voice row for machine `m` of the collection? Its type is the number the sticky assignment gave the machine's key.
pub(super) unsafe fn voices<C: CoreApi>(
    core: &C,
    rec: usize,
    b: &Build,
    keys: &KeysCopy,
) -> [bool; MAX_M] {
    let mut voice = [false; MAX_M];
    let mut m = 0;
    while m < b.nm {
        let t = assign::resolve(core, machine_key(keys, &b.machines[m]));
        voice[m] = dsp::voice_row(core, rec, t).is_some();
        m += 1;
    }
    voice
}
