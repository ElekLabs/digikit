//! The rule for the records of the machines service, as far as the bytes of the records of one kind decide it. One accumulator, [`Acc`], holds it: `Machines::validate_records`
//! (a host tool on a built module) feeds it the bodies of one kind, and the service's `check` (load) feeds it every parameter, machine, page and action record of the section in
//! file order, so the two cannot differ.
//!
//! The accumulator covers: the number of rows per kind (parameters, machines and pages as many as the registry has, at most 32 actions); the size of each body; a parameter's
//! index equal to its position, minimum not above maximum, and printable names; a page's level and printable texts; a machine's fixed fields (position 255, `own_dsp` 0 or 1,
//! level) and printable texts; a machine key at most once; an action naming a parameter number below 32, at most one action per parameter.
//!
//! What stays in `check`, because it needs the device, the module's image, another kind or the registry state: the device's slot base and knob count (`slot`, `template`), a
//! knob or action that names a parameter of the file (`pi < np`), an action's code offset inside the image, a page's machine, `own_dsp` 1 on a device without page rows, two
//! machine keys that resolve to one type number, a type number in use, the native pages and the free rows.
use dkrt_abi::mem::cstr_len;
use dkrt_abi::mem::rd32;
use dkrt_abi::service::Problem;
use dkrt_sdk::records::{
    KIND_ACTION, KIND_MACHINE, KIND_PAGE, KIND_PARAM, LEN_ACTION, LEN_MACHINE, LEN_PAGE, LEN_PARAM,
};
use dkrt_sdk::{limits, E_FORMAT};
use dkrt_svc_params::dynreg::MACH_TEXT;

/// The `what` of each problem.
pub mod what {
    pub const COUNT: &str = "more records of this kind than the registry has rows";
    pub const SIZE: &str = "the body has the wrong size";
    pub const PARAM: &str =
        "a parameter has minimum above maximum, or a name that is not printable ASCII";
    pub const PARAM_INDEX: &str = "the parameter index is not its position";
    pub const PAGE: &str =
        "a page has a level above 0xffff, or a name or title that is not printable ASCII";
    pub const MACHINE: &str = "a machine has a position other than 255, own_dsp above 1, a level above 0xffff, or a name that is not printable ASCII";
    pub const MACHINE_KEY_TWICE: &str = "same machine key as an earlier machine";
    pub const ACTION: &str = "an action names a parameter number of 32 or more";
    pub const ACTION_TWICE: &str = "a parameter has one action at most";
}

/// A NUL-terminated printable-ASCII text of exactly `field` bytes, 1 to `field - 1` characters.
pub unsafe fn text_ok(p: *const u8, field: usize) -> bool {
    let n = cstr_len(p, field);
    if n == 0 || n == field {
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

/// A PARAM record's minimum and maximum, and its two texts.
pub unsafe fn param_body(b: *const u8) -> bool {
    (rd32(b.add(12)) as i32) <= (rd32(b.add(16)) as i32)
        && text_ok(b.add(24), 32)
        && text_ok(b.add(56), 16)
}

/// A PAGE record's level and its two texts.
pub unsafe fn page_body(b: *const u8) -> bool {
    rd32(b.add(36)) <= 0xffff
        && text_ok(b.add(40), MACH_TEXT)
        && text_ok(b.add(40 + MACH_TEXT), MACH_TEXT)
}

/// A MACHINE record's fixed fields: the position of a run-time machine (255), `own_dsp` 0 or 1, the level.
pub unsafe fn machine_fields(b: *const u8) -> bool {
    rd32(b.add(8)) == 255 && rd32(b.add(12)) <= 1 && rd32(b.add(16)) <= 0xffff
}

/// A MACHINE record's two texts.
pub unsafe fn machine_texts(b: *const u8) -> bool {
    text_ok(b.add(52), MACH_TEXT) && text_ok(b.add(52 + MACH_TEXT), MACH_TEXT)
}

/// The size of the body of record kind `kind`; `None` for a kind this rule does not cover (the native pages and every kind of another service).
pub fn body_len(kind: u32) -> Option<usize> {
    let whole = if kind == KIND_PARAM {
        LEN_PARAM
    } else if kind == KIND_MACHINE {
        LEN_MACHINE
    } else if kind == KIND_PAGE {
        LEN_PAGE
    } else if kind == KIND_ACTION {
        LEN_ACTION
    } else {
        return None;
    };
    Some(whole as usize - 8)
}

fn rows(kind: u32) -> usize {
    if kind == KIND_PARAM {
        limits::PARAM_ROWS
    } else if kind == KIND_MACHINE {
        limits::MACHINE_ROWS
    } else if kind == KIND_PAGE {
        limits::PAGE_ROWS
    } else {
        32
    }
}

/// Where the counter of `kind` is in [`Acc::seen`].
fn slot(kind: u32) -> usize {
    if kind == KIND_PARAM {
        0
    } else if kind == KIND_MACHINE {
        1
    } else if kind == KIND_PAGE {
        2
    } else {
        3
    }
}

/// The rule, one record at a time: the records of a module, of any of the four kinds and in file order, go through [`Acc::record`]. It keeps what a later record needs to know about
/// the earlier ones (how many of each kind, the machine keys, the parameters that have an action). It reads nothing but its arguments and has no panic path (the service runs it on the card).
pub struct Acc {
    /// Records seen so far of each kind: parameter, machine, page, action. The count is a record's index within its kind.
    seen: [usize; 4],
    /// The key of each machine record that had the right size, by its index.
    keys: [Option<u32>; limits::MACHINE_ROWS],
    /// The parameters that have an action.
    acted: u32,
}

impl Acc {
    pub const fn new() -> Acc {
        Acc {
            seen: [0; 4],
            keys: [None; limits::MACHINE_ROWS],
            acted: 0,
        }
    }

    /// Checks the next record of `kind` (its body, after the 8-byte header) and reports each problem. A kind this rule does not cover is not looked at.
    pub fn record(&mut self, kind: u32, body: &[u8], report: &mut dyn FnMut(Problem)) {
        let Some(len) = body_len(kind) else {
            return;
        };
        let at = slot(kind);
        let index = self.seen.get(at).copied().unwrap_or(0);
        if let Some(n) = self.seen.get_mut(at) {
            *n += 1;
        }
        if index == rows(kind) {
            report(Problem::set(kind, E_FORMAT, what::COUNT));
        }
        if body.len() != len {
            report(Problem::record(kind, index, E_FORMAT, what::SIZE));
            return;
        }
        let p = body.as_ptr();
        // SAFETY: `body` has the body length of `kind`, which every read below stays inside
        unsafe {
            if kind == KIND_PARAM {
                if rd32(p) != index as u32 {
                    report(Problem::record(kind, index, E_FORMAT, what::PARAM_INDEX));
                }
                if !param_body(p) {
                    report(Problem::record(kind, index, E_FORMAT, what::PARAM));
                }
            } else if kind == KIND_PAGE {
                if !page_body(p) {
                    report(Problem::record(kind, index, E_FORMAT, what::PAGE));
                }
            } else if kind == KIND_MACHINE {
                if !machine_fields(p) || !machine_texts(p) {
                    report(Problem::record(kind, index, E_FORMAT, what::MACHINE));
                }
                let key = rd32(p);
                let mut with = 0;
                while with < index && with < self.keys.len() {
                    if self.keys.get(with).copied().flatten() == Some(key) {
                        report(Problem::clash(
                            kind,
                            index,
                            with,
                            E_FORMAT,
                            what::MACHINE_KEY_TWICE,
                        ));
                        break;
                    }
                    with += 1;
                }
                if let Some(k) = self.keys.get_mut(index) {
                    *k = Some(key);
                }
            } else {
                let pi = rd32(p);
                if pi >= 32 {
                    report(Problem::record(kind, index, E_FORMAT, what::ACTION));
                } else if self.acted & (1 << pi) != 0 {
                    report(Problem::record(kind, index, E_FORMAT, what::ACTION_TWICE));
                } else {
                    self.acted |= 1 << pi;
                }
            }
        }
    }
}

/// Checks the records `bodies` of `kind` (parameter, machine, page or action) of one module, in order. The native page kinds and a kind the service does not own are accepted.
pub fn records(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if crate::filter::is_kind(kind) {
        return crate::filter::validate(bodies.iter().map(|&body| (kind, body)), report);
    }
    let mut acc = Acc::new();
    for body in bodies {
        acc.record(kind, body, report);
    }
}
