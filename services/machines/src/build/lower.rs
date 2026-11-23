//! Lowering of the collection to records.
use super::*;
use dkrt_abi::keys::KeysCopy;
use dkrt_abi::service::RecordSink;
use dkrt_sdk::records::{
    KIND_ACTION, KIND_MACHINE, KIND_NATIVE_PAGE, KIND_NATIVE_PAGE_V2, KIND_PAGE, KIND_PARAM,
    LEN_ACTION, LEN_MACHINE, LEN_NATIVE_PAGE, LEN_NATIVE_PAGE_V2, LEN_PAGE, LEN_PARAM,
};
use dkrt_sdk::*;
use dkrt_svc_params::builder::len_of;

// ---- lowering to records -----------------------------------------------------------------------------------------------------

/// The (min, max) knob units a glyph allows (`typed.rs glyph_info`); None for an unknown glyph.
pub(super) fn glyph_range(g: u32) -> Option<(i32, i32)> {
    if g == 0 || g == 1 || g == 2 {
        Some((0, 127))
    } else if g == 3 {
        Some((0, 1))
    } else if g == 4 {
        Some((4, 124))
    } else if g == 5 {
        Some((0, 3))
    } else {
        None
    }
}

/// How the collection lowers: per machine, the knob position of each of its words, and the file index of each collected machine param.
pub struct Plan {
    /// file index (PARAM record order) of collection param i
    pub file: [u8; MAX_PA],
    pub nfile: usize,
    /// collection param index of file param k
    pub of_file: [u8; MAX_PA],
    /// knob position of collection param i
    pub pos: [u8; MAX_PA],
    /// word of collection param i within its machine
    pub word: [u8; MAX_PA],
    /// the machine a collection param belongs to
    pub machine: [u8; MAX_PA],
}

impl Plan {
    pub const fn new() -> Plan {
        Plan {
            file: [u8::MAX; MAX_PA],
            nfile: 0,
            of_file: [0; MAX_PA],
            pos: [0; MAX_PA],
            word: [0; MAX_PA],
            machine: [0; MAX_PA],
        }
    }
}

/// The key of a machine of the collection: the keys table row at its index (`check_commit` refused an index outside the table).
pub fn machine_key(keys: &KeysCopy, m: &CMachine) -> u32 {
    let i = m.index as usize;
    if i < keys.nm {
        keys.mk[i]
    } else {
        0
    }
}

/// The knob page of machine `m`: the index of its only non-canvas page, None without one; Err when it has two.
pub(super) fn knob_page(b: &Build, m: usize) -> Result<Option<usize>, i32> {
    let mut found = None;
    let mut i = 0;
    while i < b.np {
        let g = &b.pages[i];
        if g.owner as usize == m && !g.canvas {
            if found.is_some() {
                return Err(E_FORMAT); // a machine has one knob page for now
            }
            found = Some(i);
        }
        i += 1;
    }
    Ok(found)
}

/// Lower the collection into `o`: the records of PARAM, MACHINE, PAGE, ACTION and native page kinds. `o` without a buffer only measures. `base` turns callback addresses into image
/// offsets. `voice[m]`: the DSP section has a voice for machine `m` (only then may it render with its own DSP routine). The errors are refusals of the commit.
pub fn lower(
    b: &Build,
    p: &Profile,
    keys: &KeysCopy,
    voice: &[bool; MAX_M],
    base: u32,
    o: &mut RecordSink,
    plan: &mut Plan,
) -> Result<(), i32> {
    if b.err != 0 {
        return Err(b.err);
    }
    *plan = Plan::new();
    // machine params: file order is machine by machine, then word order
    let mut m = 0;
    while m < b.nm {
        let mut word = 0usize;
        let mut i = 0;
        while i < b.nparam {
            let q = &b.params[i];
            let g = &b.pages[q.page as usize];
            if g.owner as usize == m {
                if g.canvas {
                    return Err(E_FORMAT); // a canvas page binds params, it adds none
                }
                if word >= p.nslots as usize || word >= KNOBS {
                    return Err(E_FORMAT); // more params than the device has knob positions
                }
                plan.file[i] = plan.nfile as u8;
                plan.of_file[plan.nfile] = i as u8;
                plan.nfile += 1;
                plan.pos[i] = p.slots[word] as u8;
                plan.word[i] = word as u8;
                plan.machine[i] = m as u8;
                word += 1;
            }
            i += 1;
        }
        m += 1;
    }
    // PARAM
    let mut k = 0;
    while k < plan.nfile {
        let i = plan.of_file[k] as usize;
        let q = &b.params[i];
        let (gmin, gmax) = match glyph_range(q.glyph) {
            Some(r) => r,
            None => return Err(E_FORMAT),
        };
        if q.min < gmin * 256 || q.max > gmax * 256 {
            return Err(E_FORMAT); // the range does not fit the glyph
        }
        let tpl = p.tpl[q.glyph as usize];
        o.head(KIND_PARAM, LEN_PARAM as usize);
        o.p32(k as u32);
        o.p32(tpl);
        o.p32(p.slot_base + plan.pos[i] as u32);
        o.p32(q.min as u32);
        o.p32(q.max as u32);
        o.p32(q.def as u32);
        o.text(&q.name, 32);
        o.text(&q.short, 16);
        k += 1;
    }
    // MACHINE, then the knob page of each (page devices)
    let mut m = 0;
    while m < b.nm {
        let mc = &b.machines[m];
        let mut knobs = [0u32; KNOBS];
        if !p.pages {
            if p.tune_code != 0 && (p.tune_slot as usize) < KNOBS {
                knobs[p.tune_slot as usize] = p.tune_code;
            }
            if p.level_code != 0 && (p.level_knob as usize) < KNOBS {
                knobs[p.level_knob as usize] = p.level_code;
            }
            let mut i = 0;
            while i < b.nparam {
                if plan.file[i] != u8::MAX && plan.machine[i] as usize == m {
                    knobs[plan.pos[i] as usize] = KNOB_PARAM | plan.file[i] as u32;
                }
                i += 1;
            }
        }
        o.head(KIND_MACHINE, LEN_MACHINE as usize);
        o.p32(machine_key(keys, mc));
        o.p32(p.clone_of);
        o.p32(POS_END);
        o.p32((p.own_dsp && voice[m]) as u32);
        o.p32(p.level);
        let mut j = 0;
        while j < KNOBS {
            o.p32(knobs[j]);
            j += 1;
        }
        o.text(&mc.name, TEXT);
        o.text(&mc.short, TEXT);
        m += 1;
    }
    if p.pages {
        let mut m = 0;
        while m < b.nm {
            let kp = knob_page(b, m)?;
            let mut knobs = [0u32; KNOBS];
            let mut i = 0;
            while i < b.nparam {
                if plan.file[i] != u8::MAX && plan.machine[i] as usize == m {
                    knobs[plan.pos[i] as usize] = KNOB_PARAM | plan.file[i] as u32;
                }
                i += 1;
            }
            let mc = &b.machines[m];
            // the page's own name and short name; a machine without a knob page gets its name in capitals as both
            let mut upper = [0u8; TEXT];
            let (name, tab): (&[u8], &[u8]) = match kp {
                Some(g) => (&b.pages[g].name, &b.pages[g].short),
                None => {
                    let n = len_of(&mc.name);
                    let mut j = 0;
                    while j < n {
                        upper[j] = mc.name[j].to_ascii_uppercase();
                        j += 1;
                    }
                    (&mc.name, &upper)
                }
            };
            o.head(KIND_PAGE, LEN_PAGE as usize);
            o.p32(machine_key(keys, mc));
            let mut j = 0;
            while j < KNOBS {
                o.p32(knobs[j]);
                j += 1;
            }
            o.p32(p.level);
            o.text(tab, TEXT);
            o.text(name, TEXT);
            m += 1;
        }
    } else {
        // the page of a machine-row device still must be unique
        let mut m = 0;
        while m < b.nm {
            knob_page(b, m)?;
            m += 1;
        }
    }
    // ACTION
    let mut i = 0;
    while i < b.nparam {
        let q = &b.params[i];
        if q.click != 0 && plan.file[i] != u8::MAX {
            o.head(KIND_ACTION, LEN_ACTION as usize);
            o.p32(plan.file[i] as u32);
            o.p32(q.click.wrapping_sub(base));
        }
        i += 1;
    }
    // native pages
    let mut m = 0;
    while m < b.nm {
        let mut canvas = [0usize; MAX_PG];
        let mut n = 0;
        let mut i = 0;
        while i < b.np {
            let g = &b.pages[i];
            if g.owner as usize == m && g.canvas {
                canvas[n] = i;
                n += 1;
            }
            i += 1;
        }
        let v1 = n == 1 && b.pages[canvas[0]].flags == 0;
        let mut taken = 0u32;
        let mut j = 0;
        while j < n {
            let g = &b.pages[canvas[j]];
            let index = if g.flags & 1 != 0 {
                0
            } else if g.flags & 2 != 0 {
                1
            } else if v1 {
                1
            } else if taken & 1 == 0 {
                0
            } else {
                1
            };
            taken |= 1 << index;
            if v1 {
                o.head(KIND_NATIVE_PAGE, LEN_NATIVE_PAGE as usize);
                o.p32(1);
            } else {
                o.head(KIND_NATIVE_PAGE_V2, LEN_NATIVE_PAGE_V2 as usize);
                o.p32(2);
            }
            o.p32(machine_key(keys, &b.machines[m]));
            o.p32(g.draw.wrapping_sub(base));
            o.p32(g.event.wrapping_sub(base));
            o.text(&g.name, TEXT);
            if !v1 {
                o.p32(index);
                o.p32(g.flags);
            } else {
                // v1 has the binding table at byte 40 of the body: the title ends there
            }
            let mut bi = 0;
            while bi < 8 {
                if bi < g.nbind as usize {
                    let [kind, value, slot] = g.binds[bi];
                    let v = if kind == 1 {
                        let i = value as usize;
                        if i >= b.nparam || plan.file[i] == u8::MAX {
                            return Err(E_FORMAT);
                        }
                        plan.file[i] as u32
                    } else {
                        value
                    };
                    o.p32(kind);
                    o.p32(v);
                    o.p32(slot);
                } else {
                    o.p32(0);
                    o.p32(0);
                    o.p32(0);
                }
                bi += 1;
            }
            j += 1;
        }
        m += 1;
    }
    Ok(())
}
