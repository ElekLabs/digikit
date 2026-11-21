//! The rule every LFO shape module must meet, in one place: `Lfo::validate_records` runs it. The service's `check` calls it at load, and `patchkit records --check` calls it on a
//! built module, so the two cannot differ.
//!
//! [`body_ok`] is the rule of one record (a key, a name of four printable characters). [`check_set`] adds what only a whole module can break: the number of shapes, names the MOD
//! page cannot tell apart (the short formatter shows the first three characters), and a shape that does nothing as an LFO (flat). It reads no state and has no panic path.
use crate::records::{MAX_SHAPES, NAME_LEN, SAMPLES};
use dkrt_abi::service::Problem;
use dkrt_sdk::records::{KIND_LFO_SHAPE, LEN_LFO_SHAPE};
use dkrt_sdk::{E_FORMAT, E_TYPE_USED};

/// Bytes of a shape record's body: the key, the name and the samples (the record is [`LEN_LFO_SHAPE`] bytes with its 8-byte header).
pub const BODY_LEN: usize = LEN_LFO_SHAPE as usize - 8;
/// Characters of a name that the short formatter shows.
pub const SHORT_LEN: usize = 3;

/// The `what` of each problem.
pub mod what {
    pub const COUNT: &str = "a module has 1 to 8 shapes";
    pub const SIZE: &str = "the body is not 72 bytes";
    pub const RECORD: &str = "key 0, or a name character outside space to tilde";
    pub const NAME_TWICE: &str = "same name as an earlier shape";
    pub const SHORT_NAME_TWICE: &str =
        "first 3 characters equal those of an earlier shape (the short formatter shows only 3)";
    pub const KEY_TWICE: &str = "same key as an earlier shape";
    pub const FLAT: &str = "all samples are equal";
    pub const ZERO: &str = "all samples are 0";
}

/// The rule of one record: a key other than 0 and a name of four printable characters (space to tilde). `body` is at least [`BODY_LEN`] bytes.
pub fn body_ok(body: &[u8]) -> bool {
    // no indexing: the service runs this on the card, where a bounds check would be a panic path
    let (Some(key), Some(name)) = (body.get(0..4), body.get(4..4 + NAME_LEN)) else {
        return false;
    };
    key.iter().any(|&c| c != 0) && name.iter().all(|c| (0x20..=0x7e).contains(c))
}

fn word(body: &[u8], i: usize) -> Option<i16> {
    Some(i16::from_be_bytes([
        *body.get(8 + 2 * i)?,
        *body.get(9 + 2 * i)?,
    ]))
}

fn flat(body: &[u8]) -> Option<i16> {
    let first = word(body, 0)?;
    let mut i = 1;
    while i < SAMPLES {
        if word(body, i)? != first {
            return None;
        }
        i += 1;
    }
    Some(first)
}

fn same(a: &[u8], b: &[u8], n: usize) -> bool {
    match (a.get(4..4 + n), b.get(4..4 + n)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// Checks the bodies of one module's shape records, in order, and calls `report` for each problem.
pub fn check_set(bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    const K: u32 = KIND_LFO_SHAPE;
    if bodies.is_empty() || bodies.len() > MAX_SHAPES {
        report(Problem::set(K, E_FORMAT, what::COUNT));
    }
    let mut index = 0;
    while index < bodies.len() {
        let b = bodies[index];
        if b.len() != BODY_LEN {
            report(Problem::record(K, index, E_FORMAT, what::SIZE));
            index += 1;
            continue;
        }
        let printable = b[4..4 + NAME_LEN].iter().all(|c| (0x20..=0x7e).contains(c));
        if !body_ok(b) {
            report(Problem::record(K, index, E_FORMAT, what::RECORD));
        }
        let mut of = 0;
        while of < index {
            let o = bodies[of];
            if o.len() == BODY_LEN {
                if o[0..4] == b[0..4] {
                    report(Problem::clash(K, index, of, E_TYPE_USED, what::KEY_TWICE));
                }
                // an unprintable name is reported once, by `RECORD`
                if printable && same(o, b, NAME_LEN) {
                    report(Problem::clash(K, index, of, E_FORMAT, what::NAME_TWICE));
                } else if printable && same(o, b, SHORT_LEN) {
                    report(Problem::clash(
                        K,
                        index,
                        of,
                        E_FORMAT,
                        what::SHORT_NAME_TWICE,
                    ));
                }
            }
            of += 1;
        }
        if let Some(first) = flat(b) {
            let (code, w) = (E_FORMAT, if first == 0 { what::ZERO } else { what::FLAT });
            report(Problem::record(K, index, code, w));
        }
        index += 1;
    }
}
