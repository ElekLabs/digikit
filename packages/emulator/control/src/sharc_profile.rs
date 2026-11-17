//! The answer of `sharc.profile`: DSP instruction counts as raw counters of the profile artifact `dk.profile/1` (packages/dkprof).
//!
//! The engine keeps two sources, both counted in executed instructions and keyed by software (short-word) PC:
//! - the dispatch profile of the fast and block tiers (engine feature `region-profile`): the instructions of each dispatch, credited to the
//!   entry PC of the dispatch (a region or a block, so a count at an entry PC covers the whole region that starts there);
//! - the interpreter's histogram (`SHARC_FAST_HIST`): one count per interpreted instruction, at its own PC.
//!
//! The two are added into one address-keyed series `pc`. Nothing reads EMUCLK or TCOUNT (finding 125: a clock read can change the tier).

use serde_json::{Value, json};
use std::collections::BTreeMap;

/// What the engine counted, as plain numbers.
#[derive(Clone, Debug, Default)]
pub struct SharcCounts {
    /// DSP blocks (frames) the worker handled since it started.
    pub blocks: u64,
    /// Instructions executed in total, and replayed by the idle skip (not counted per PC).
    pub icount: u64,
    pub idle_skipped: u64,
    /// `(entry pc, fast-tier instructions, block-tier instructions)` of every dispatched entry.
    pub regions: Vec<(u32, u64, u64)>,
    /// `(pc, interpreted instructions)`.
    pub hist: Vec<(u32, u64)>,
}

pub fn profile_json(c: &SharcCounts) -> Value {
    let mut by_pc: BTreeMap<u32, u64> = BTreeMap::new();
    let (mut fast, mut block, mut interpreted) = (0u64, 0u64, 0u64);
    for &(pc, f, b) in &c.regions {
        fast += f;
        block += b;
        *by_pc.entry(pc).or_default() += f + b;
    }
    for &(pc, n) in &c.hist {
        interpreted += n;
        *by_pc.entry(pc).or_default() += n;
    }
    let raw: Vec<Value> = by_pc
        .into_iter()
        .filter(|(_, n)| *n > 0)
        .map(|(pc, n)| json!({"cpu": "sharc", "addr": pc, "count": n, "series": "pc"}))
        .collect();
    json!({
        "clock": {
            "source": "emu.sharc.insn",
            "unit": "insn",
            "sampling": "pc: executed instructions; a fast-tier or block-tier dispatch is credited whole to its entry PC, an interpreted instruction to its own PC; idle-skipped instructions are not counted",
        },
        "blocks": c.blocks,
        "icount": c.icount,
        "idle_skipped": c.idle_skipped,
        "counted": {"fast": fast, "block": block, "interpreted": interpreted},
        "raw": raw,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_answer_adds_regions_and_interpreter_counts_by_pc_and_names_what_it_counted() {
        let c = SharcCounts {
            blocks: 3,
            icount: 1000,
            idle_skipped: 400,
            regions: vec![(0x100, 50, 0), (0x200, 0, 30), (0x300, 0, 0)],
            hist: vec![(0x100, 5), (0x180, 2)],
        };
        let v = profile_json(&c);
        assert_eq!(v["clock"]["unit"], "insn");
        assert_eq!(v["blocks"], 3);
        assert_eq!(v["counted"]["fast"], 50);
        assert_eq!(v["counted"]["block"], 30);
        assert_eq!(v["counted"]["interpreted"], 7);
        let raw: Vec<(u64, u64)> = v["raw"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| {
                assert_eq!(r["cpu"], "sharc");
                assert_eq!(r["series"], "pc");
                (r["addr"].as_u64().unwrap(), r["count"].as_u64().unwrap())
            })
            .collect();
        // sorted by address; an entry with no instructions is left out
        assert_eq!(raw, vec![(0x100, 55), (0x180, 2), (0x200, 30)]);
    }
}
