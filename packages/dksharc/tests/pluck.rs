//! The oracle: the DSP section of the pluck machine for the Digitone II (our own module) and its DSP object. The numbers are those of
//! the Python P3a run (instructions, functions, loops, calls) that the Rust crate replaces.

use dksharc::*;

const DKDS: &[u8] = include_bytes!("fixtures/pluck-dn2.dkds");
const DKO: &[u8] = include_bytes!("fixtures/pluck-dsp.dko.json");

fn pluck() -> Analysis {
    analyze(&Input::from_dkds(DKDS, DKO).unwrap())
}

#[test]
fn counts_match_the_python_oracle() {
    let a = pluck();
    assert_eq!(a.summary.instructions, 306);
    assert_eq!(a.insns.len(), 306);
    assert_eq!(a.summary.functions, 3);
    assert_eq!(a.summary.loops, 4);
    assert_eq!(a.summary.calls, 6);
    assert_eq!(a.summary.import_calls, 4);
    assert_eq!(a.summary.undecoded, 0);
    assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
    // The instructions tile the text: no gap, no overlap.
    let mut pc = a.base_sw;
    for i in &a.insns {
        assert_eq!(i.pc, pc);
        pc += i.parcels as u32;
    }
    assert_eq!(pc - a.base_sw, a.text_parcels);
}

#[test]
fn functions() {
    let a = pluck();
    let f: Vec<_> = a
        .functions
        .iter()
        .map(|f| (f.name.as_str(), f.entry - a.base_sw, f.global, f.complete))
        .collect();
    assert_eq!(
        f,
        [
            ("_pk_coef", 0, false, true),
            ("_pk_note_on", 253, true, true),
            ("_pk_render", 487, true, true)
        ]
    );
    assert_eq!(a.functions[0].origin, ["text_start", "call"]);
    assert_eq!(a.functions[1].origin, ["symbol", "entry:note_on"]);
    assert_eq!(a.functions[2].origin, ["symbol", "entry:render"]);
    assert_eq!(a.functions[2].end - a.base_sw, 664);
}

#[test]
fn loops() {
    let a = pluck();
    let l: Vec<_> = a
        .loops
        .iter()
        .map(|l| (l.do_pc - a.base_sw, l.function, &l.count, l.depth))
        .collect();
    assert_eq!(l.len(), 4);
    assert_eq!(
        l.iter()
            .filter(|l| *l.2 == LoopCount::Lit { count: 32 })
            .count(),
        1
    );
    assert_eq!(
        l.iter()
            .filter(|l| matches!(l.2, LoopCount::Reg { .. }))
            .count(),
        3
    );
    // The literal loop is in _pk_render; the three register loops in _pk_note_on; none is nested.
    assert_eq!(
        l.iter()
            .find(|l| matches!(l.2, LoopCount::Lit { .. }))
            .unwrap()
            .1,
        Some(2)
    );
    assert!(
        l.iter()
            .filter(|l| matches!(l.2, LoopCount::Reg { .. }))
            .all(|l| l.1 == Some(1))
    );
    assert!(l.iter().all(|l| l.3 == 0));
}

#[test]
fn calls() {
    let a = pluck();
    let labels: Vec<String> = a.calls.iter().map(|c| c.label(&a)).collect();
    assert_eq!(
        labels,
        [
            "import:_dv_exp2",
            "import:_dv_exp2",
            "import:_dv_exp2",
            "import:_dv_exp2",
            "_pk_coef",
            "_pk_coef"
        ]
    );
    // The import calls are in _pk_coef; the module's own calls to it come from the other two functions.
    assert_eq!(
        a.calls.iter().map(|c| c.caller).collect::<Vec<_>>(),
        [Some(0), Some(0), Some(0), Some(0), Some(1), Some(2)]
    );
}

#[test]
fn blocks_are_consistent() {
    let a = pluck();
    for (fi, f) in a.functions.iter().enumerate() {
        // Blocks tile the function.
        let mut pc = f.entry;
        for b in &f.blocks {
            assert_eq!(b.start, pc);
            assert!(b.end > b.start);
            pc = b.end;
            for e in &b.succ {
                assert!(
                    a.functions[fi].blocks.iter().any(|t| t.start == e.to),
                    "{}: edge to {:#x} is no block start",
                    f.name,
                    e.to
                );
            }
        }
        assert_eq!(pc, f.end);
    }
    // Every loop has a loop-back edge from its end to its start.
    for l in &a.loops {
        let f = &a.functions[l.function.unwrap()];
        assert!(f.blocks.iter().any(|b| {
            b.succ
                .iter()
                .any(|e| e.kind == EdgeKind::LoopBack && e.to == l.start)
        }));
    }
}

#[test]
fn memory_operands() {
    let a = pluck();
    assert_eq!(a.summary.memory_operands, 65);
    let ops: Vec<&MemOp> = a.insns.iter().flat_map(|i| &i.mem).collect();
    // The two direct accesses, `DM(<addr>)`.
    assert_eq!(ops.iter().filter(|m| m.mode == Mode::Direct).count(), 2);
    assert!(
        ops.iter()
            .filter(|m| m.mode == Mode::Direct)
            .all(|m| m.addr.is_some() && m.i.is_none())
    );
    // A post-modify load through I1 and M5 (Type 3a) exists.
    assert!(ops.iter().any(|m| m.mode == Mode::PostModify
        && m.dir == Direction::Load
        && m.i == Some(1)
        && m.m == Some(5)));
    // Everything is DM here; the register and offset forms carry their index register.
    assert!(ops.iter().all(|m| m.space == Space::Dm));
    assert!(
        ops.iter()
            .filter(|m| m.mode == Mode::PreModifyImm)
            .all(|m| m.i.is_some() && m.imm.is_some())
    );
}

#[test]
fn json_round_trip() {
    let a = pluck();
    let json = serde_json::to_string(&a).unwrap();
    let back: Analysis = serde_json::from_str(&json).unwrap();
    assert_eq!(a, back);
}

#[test]
fn bad_input_is_an_error() {
    let mut bad = DKDS.to_vec();
    bad[0] = 0;
    assert!(Input::from_dkds(&bad, DKO).unwrap_err().contains("magic"));
    assert!(Input::from_dkds(&DKDS[..0x30], DKO).is_err());
    assert!(Input::from_dkds(DKDS, b"{}").is_err());
}
