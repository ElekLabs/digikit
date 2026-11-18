//! The rule for the records of the machines service, as far as a record's own bytes decide it: shared by the service's `check` (load) and `Machines::validate_records`
//! (a host tool on a built module). What needs the device or another record stays in `check`: the device's slot base and knob count (`slot`, `template`), a knob that names
//! a parameter of the file, a page's machine, the native pages (their code offsets depend on the module's image), and the free rows.
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

fn body_len(kind: u32) -> usize {
    (if kind == KIND_PARAM {
        LEN_PARAM
    } else if kind == KIND_MACHINE {
        LEN_MACHINE
    } else if kind == KIND_PAGE {
        LEN_PAGE
    } else {
        LEN_ACTION
    }) as usize
        - 8
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

/// Checks the records `bodies` of `kind` (parameter, machine, page or action) of one module, in order. The native page kinds and a kind the service does not own are accepted.
pub fn records<R: FnMut(Problem)>(kind: u32, bodies: &[&[u8]], report: &mut R) {
    if kind != KIND_PARAM && kind != KIND_MACHINE && kind != KIND_PAGE && kind != KIND_ACTION {
        return;
    }
    if bodies.len() > rows(kind) {
        report(Problem::set(kind, E_FORMAT, what::COUNT));
    }
    let mut acted = 0u32;
    let mut index = 0;
    while index < bodies.len() {
        let b = bodies[index];
        if b.len() != body_len(kind) {
            report(Problem::record(kind, index, E_FORMAT, what::SIZE));
            index += 1;
            continue;
        }
        let p = b.as_ptr();
        // SAFETY: `b` has the body length of `kind`, which every read below stays inside
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
                let mut with = 0;
                while with < index {
                    if bodies[with].len() == b.len() && rd32(bodies[with].as_ptr()) == rd32(p) {
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
            } else {
                let pi = rd32(p);
                if pi >= 32 {
                    report(Problem::record(kind, index, E_FORMAT, what::ACTION));
                } else if acted & (1 << pi) != 0 {
                    report(Problem::record(kind, index, E_FORMAT, what::ACTION_TWICE));
                } else {
                    acted |= 1 << pi;
                }
            }
        }
        index += 1;
    }
}
