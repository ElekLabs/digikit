//! The rule for the effect records of one module, shared by the service's `check` (load) and `Fx::validate_records` (a host tool on a built module).
//! One record is checked by `fxpage::check_body`; the set must not hold more effects than the chain takes, and no effect index twice.
use crate::fxpage::{check_body, KIND_EFFECT, LEN_EFFECT};
use dkrt_abi::dsp::MAX_FX;
use dkrt_abi::mem::rd32;
use dkrt_abi::service::Problem;
use dkrt_sdk::E_FORMAT;

/// Bytes of an effect record's body (the record is `LEN_EFFECT` bytes with its 8-byte header).
pub const BODY_LEN: usize = LEN_EFFECT as usize - 8;

/// The `what` of each problem.
pub mod what {
    pub const COUNT: &str = "more effects than the chain takes";
    pub const SIZE: &str = "the body has the wrong size";
    pub const RECORD: &str =
        "a field is out of range, a text is not printable ASCII, or a knob is wrong";
    pub const FX_TWICE: &str = "same effect index as an earlier effect";
}

/// Checks the effect records `bodies` of one module, in order; a kind other than the effect record is accepted.
pub fn effects(kind: u32, bodies: &[&[u8]], report: &mut dyn FnMut(Problem)) {
    if kind != KIND_EFFECT {
        return;
    }
    if bodies.len() > MAX_FX {
        report(Problem::set(kind, E_FORMAT, what::COUNT));
    }
    let mut seen = 0u32;
    let mut index = 0;
    while index < bodies.len() {
        let b = bodies[index];
        if b.len() != BODY_LEN {
            report(Problem::record(kind, index, E_FORMAT, what::SIZE));
        } else {
            // SAFETY: `b` has `BODY_LEN` bytes, all the record reads
            if unsafe { check_body(b.as_ptr()) } != dkrt_sdk::OK {
                report(Problem::record(kind, index, E_FORMAT, what::RECORD));
            } else {
                let fx = unsafe { rd32(b.as_ptr()) };
                if fx < 32 && seen & (1 << fx) != 0 {
                    // the earlier record with this index
                    let mut with = 0;
                    while with < index
                        && !(bodies[with].len() == BODY_LEN
                            && unsafe { rd32(bodies[with].as_ptr()) } == fx)
                    {
                        with += 1;
                    }
                    report(Problem::clash(kind, index, with, E_FORMAT, what::FX_TWICE));
                }
                if fx < 32 {
                    seen |= 1 << fx;
                }
            }
        }
        index += 1;
    }
}
