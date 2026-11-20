//! The rule every module's shortcut records must meet, in one place: `ShortcutsService::validate_records` runs it. The service's `check` calls it at load, and
//! `patchkit records --check` calls it on a built module, so the two cannot differ.
//!
//! A record is the body of `[records.kinds]` SHORTCUT: `id[16]`, `label[24]`, two logical keys, the offsets of `on` and `off` in the module image (0: none) and `arg`, all
//! words big-endian. [`check_set`] judges the records of one module.
use crate::bindings::{ID_BYTES, LABEL_BYTES};
use dkrt_abi::platform::KEY_SLOTS;
use dkrt_abi::service::Problem;
use dkrt_sdk::records::{KIND_SHORTCUT, LEN_SHORTCUT};
use dkrt_sdk::E_FORMAT;

/// Bytes of a record's body (the record is [`LEN_SHORTCUT`] bytes with its 8-byte header).
pub const BODY_LEN: usize = LEN_SHORTCUT as usize - 8;
/// Shortcuts of one module.
pub const MAX_PER_MODULE: usize = 4;

/// Where the fields are in the body.
pub mod at {
    pub const ID: usize = 0;
    pub const LABEL: usize = 16;
    pub const KEY0: usize = 40;
    pub const KEY1: usize = 44;
    pub const ON: usize = 48;
    pub const OFF: usize = 52;
    pub const ARG: usize = 56;
}

/// The `what` of each problem.
pub mod what {
    pub const COUNT: &str = "a module has 4 shortcuts at most";
    pub const SIZE: &str = "the body is not 60 bytes";
    pub const ID: &str =
        "the id is empty or has a character other than letters, digits, _, - and .";
    pub const LABEL: &str = "the label is empty or has a character outside space to tilde";
    pub const KEYS: &str =
        "a key is DK_KEY_NONE or outside the key table, or both keys are the same";
    pub const ON: &str = "no `on` callback";
    pub const ID_TWICE: &str = "same id as an earlier shortcut";
    pub const KEYS_TWICE: &str = "same keys as an earlier shortcut of the module";
}

fn word(body: &[u8], at: usize) -> Option<u32> {
    let b = body.get(at..at + 4)?;
    Some(u32::from_be_bytes([
        *b.first()?,
        *b.get(1)?,
        *b.get(2)?,
        *b.get(3)?,
    ]))
}

/// The text of a NUL padded field: its characters up to the first NUL, `None` when the field has no NUL (no room) or is not in `body`.
fn text(body: &[u8], at: usize, field: usize) -> Option<&[u8]> {
    let f = body.get(at..at + field)?;
    let n = f.iter().position(|c| *c == 0)?;
    let (s, rest) = f.split_at(n);
    if rest.iter().all(|c| *c == 0) {
        Some(s)
    } else {
        None
    }
}

fn id_ok(s: &[u8]) -> bool {
    !s.is_empty()
        && s.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(*c, b'_' | b'-' | b'.'))
}

fn label_ok(s: &[u8]) -> bool {
    !s.is_empty() && s.iter().all(|c| (0x20..=0x7e).contains(c))
}

/// The keys of a record, when both are logical keys of the table and differ.
pub fn keys_of(body: &[u8]) -> Option<[u8; 2]> {
    let (a, b) = (word(body, at::KEY0)?, word(body, at::KEY1)?);
    let ok = |k: u32| k != 0 && (k as usize) < KEY_SLOTS;
    if ok(a) && ok(b) && a != b {
        Some([a as u8, b as u8])
    } else {
        None
    }
}

/// The rule of one record (the `index`th of the module), reporting its problems.
pub fn body_ok(index: usize, body: &[u8], report: &mut dyn FnMut(Problem)) -> bool {
    let mut ok = true;
    let mut bad = |what: &'static str| {
        report(Problem::record(KIND_SHORTCUT, index, E_FORMAT, what));
        ok = false;
    };
    if body.len() != BODY_LEN {
        bad(what::SIZE);
        return false;
    }
    if !text(body, at::ID, ID_BYTES).is_some_and(id_ok) {
        bad(what::ID);
    }
    if !text(body, at::LABEL, LABEL_BYTES).is_some_and(label_ok) {
        bad(what::LABEL);
    }
    if keys_of(body).is_none() {
        bad(what::KEYS);
    }
    if word(body, at::ON).unwrap_or(0) == 0 {
        bad(what::ON);
    }
    ok
}

/// The rule for the records of one module: each record, then what only the whole set can break (the count, an id or a key pair twice).
pub fn check_set(bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if bodies.len() > MAX_PER_MODULE {
        report(Problem::set(
            KIND_SHORTCUT,
            dkrt_sdk::E_DECL_FULL,
            what::COUNT,
        ));
    }
    let mut i = 0;
    while i < bodies.len() {
        if body_ok(i, bodies[i], report) {
            let mut j = 0;
            while j < i {
                if text(bodies[i], at::ID, ID_BYTES) == text(bodies[j], at::ID, ID_BYTES) {
                    report(Problem::clash(
                        KIND_SHORTCUT,
                        i,
                        j,
                        E_FORMAT,
                        what::ID_TWICE,
                    ));
                } else if keys_of(bodies[i]) == keys_of(bodies[j]) {
                    report(Problem::clash(
                        KIND_SHORTCUT,
                        i,
                        j,
                        E_FORMAT,
                        what::KEYS_TWICE,
                    ));
                }
                j += 1;
            }
        }
        i += 1;
    }
}
