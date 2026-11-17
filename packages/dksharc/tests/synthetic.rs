//! Synthetic text: the structures the pluck oracle does not have (nested loops, delay slots, undecodable words).

use dksharc::*;

/// A 48-bit instruction as three parcels in boot order.
fn p48(f: u64) -> Vec<u16> {
    vec![(f >> 32) as u16, (f >> 16) as u16, f as u16]
}
fn nop() -> Vec<u16> {
    p48(0)
}
const ALWAYS: u64 = 0x1f << 33;
/// `DO <end> UNTIL LCE` with a literal count, the end given as the offset of its last instruction from the DO.
fn do_imm(count: u64, offset: u64) -> Vec<u16> {
    p48(0x0c00_0000_0000 | count << 24 | offset)
}
/// `DO <end> UNTIL LCE`, count from the Ureg code.
fn do_ureg(ureg: u64, offset: u64) -> Vec<u16> {
    p48(0x0d00_0000_0000 | ureg << 32 | offset)
}
/// `JUMP addr`, optionally `(DB)`.
fn jump(addr: u32, delayed: bool) -> Vec<u16> {
    p48(0x0600_0000_0000 | ALWAYS | (delayed as u64) << 26 | addr as u64)
}
/// `CALL addr`.
fn call(addr: u32) -> Vec<u16> {
    p48(0x0600_0000_0000 | ALWAYS | 1 << 39 | addr as u64)
}
fn rts() -> Vec<u16> {
    vec![0x0abe]
}

fn build(parts: &[Vec<u16>]) -> Analysis {
    let text: Vec<u16> = parts.concat();
    analyze(&Input {
        text,
        base_sw: L2_SW_BASE,
        symbols: vec![TextSymbol {
            name: "_f".into(),
            parcel: 0,
            global: true,
        }],
        ..Input::default()
    })
}
const B: u32 = L2_SW_BASE;

#[test]
fn nested_loops() {
    // 0 DO(outer, 4) | 3 NOP | 6 DO(inner, R5) | 9 NOP | 12 NOP (inner end) | 15 NOP (outer end) | 18 RTS
    let a = build(&[
        do_imm(4, 15),
        nop(),
        do_ureg(5, 6),
        nop(),
        nop(),
        nop(),
        rts(),
    ]);
    assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
    assert_eq!(a.loops.len(), 2);
    let (outer, inner) = (&a.loops[0], &a.loops[1]);
    assert_eq!(
        (outer.start, outer.end, outer.depth, outer.parent),
        (B + 3, B + 15, 0, None)
    );
    assert_eq!(outer.count, LoopCount::Lit { count: 4 });
    assert_eq!(
        (inner.start, inner.end, inner.depth, inner.parent),
        (B + 9, B + 12, 1, Some(0))
    );
    assert_eq!(
        inner.count,
        LoopCount::Reg {
            ureg: 5,
            name: "R5".into()
        }
    );
    let f = &a.functions[0];
    let blocks: Vec<_> = f.blocks.iter().map(|b| (b.start - B, b.end - B)).collect();
    assert_eq!(blocks, [(0, 3), (3, 9), (9, 15), (15, 18), (18, 19)]);
    let back = |s: u32| f.blocks.iter().find(|b| b.start == s).unwrap().succ.clone();
    assert_eq!(
        back(B + 9),
        [
            Edge {
                to: B + 15,
                kind: EdgeKind::Fall
            },
            Edge {
                to: B + 9,
                kind: EdgeKind::LoopBack
            }
        ]
    );
    assert_eq!(
        back(B + 15),
        [
            Edge {
                to: B + 18,
                kind: EdgeKind::Fall
            },
            Edge {
                to: B + 3,
                kind: EdgeKind::LoopBack
            }
        ]
    );
}

#[test]
fn delayed_jump_keeps_its_slots_in_the_block() {
    // 0 JUMP (DB) 12 | 3 NOP | 6 NOP | 9 NOP | 12 RTS
    let a = build(&[jump(B + 12, true), nop(), nop(), nop(), rts()]);
    assert!(a.diagnostics.is_empty(), "{:?}", a.diagnostics);
    let blocks = &a.functions[0].blocks;
    assert_eq!(blocks.len(), 3);
    let b0 = &blocks[0];
    assert_eq!((b0.start - B, b0.end - B, b0.term_pc), (0, 9, Some(B)));
    assert_eq!(b0.delay_slots, [B + 3, B + 6]);
    assert_eq!(
        b0.term,
        Terminator::Jump {
            target: Some(B + 12),
            conditional: false,
            delayed: true
        }
    );
    assert_eq!(
        b0.succ,
        [Edge {
            to: B + 12,
            kind: EdgeKind::Taken
        }]
    );
    // The instruction after the slots is unreachable from the jump and starts its own block.
    assert_eq!((blocks[1].start - B, blocks[1].end - B), (9, 12));
    assert_eq!(
        blocks[2].term,
        Terminator::Return {
            conditional: false,
            delayed: false
        }
    );
}

#[test]
fn branch_into_a_delay_slot_is_reported() {
    let a = build(&[
        jump(B + 12, true),
        nop(),
        nop(),
        nop(),
        jump(B + 3, false),
        rts(),
    ]);
    assert!(
        a.diagnostics.iter().any(|d| d.contains("delay slot")),
        "{:?}",
        a.diagnostics
    );
}

#[test]
fn call_inside_the_module_makes_a_function() {
    // 0 CALL 7 | 3 RTS | 4 NOP | 7 NOP (the callee has no symbol: a sub_ name, origin call) | 10 RTS
    let a = build(&[call(B + 7), rts(), nop(), nop(), rts()]);
    assert_eq!(a.functions.len(), 2);
    assert_eq!(a.functions[1].origin, ["call"]);
    assert_eq!(a.functions[1].entry, B + 7);
    assert!(a.functions[1].name.starts_with("sub_"));
    assert_eq!(a.calls[0].target, CallTarget::Local { pc: B + 7 });
    assert_eq!(
        a.functions[0].blocks[0].succ,
        [Edge {
            to: B + 3,
            kind: EdgeKind::ReturnSite
        }]
    );
}

#[test]
fn import_call_is_resolved_by_relocation() {
    let mut input = Input {
        text: [call(0), rts()].concat(),
        base_sw: L2_SW_BASE,
        symbols: vec![TextSymbol {
            name: "_f".into(),
            parcel: 0,
            global: true,
        }],
        imports: vec!["_dv_exp2".into()],
        relocs: vec![TextReloc {
            parcel: 0,
            kind: 0,
            class: 2,
            import: 0,
        }],
        ..Input::default()
    };
    let a = analyze(&input);
    assert_eq!(a.calls[0].label(&a), "import:_dv_exp2");
    assert_eq!(a.summary.functions, 1);
    // Without the relocation the call is a direct call to address 0: outside the text.
    input.relocs.clear();
    let a = analyze(&input);
    assert_eq!(a.calls[0].target, CallTarget::Outside { pc: 0 });
}

#[test]
fn unknown_word_is_reported_not_guessed() {
    // 0x0201 decodes to no form; the sweep reports it and goes on at the next parcel.
    let a = build(&[nop(), vec![0x0201], nop(), rts()]);
    let u: Vec<_> = a
        .undecoded
        .iter()
        .map(|u| (u.pc - B, u.decode.as_str(), u.parcels[0]))
        .collect();
    assert_eq!(u, [(3, "unknown", 0x0201)]);
    assert!(!a.functions[0].complete);
    assert_eq!(a.summary.undecoded, 1);
    // The word is no instruction of the model.
    assert!(a.insns.iter().all(|i| i.pc != B + 3));
}

#[test]
fn uncertain_decode_is_reported_and_skipped_by_its_length() {
    // 0x0002 is the undocumented 16-bit form 21p_undoc16: a form, but not a confident one.
    let a = build(&[nop(), vec![0x0002], nop(), rts()]);
    assert_eq!(a.undecoded.len(), 1);
    assert_eq!(
        (
            a.undecoded[0].pc - B,
            a.undecoded[0].decode.as_str(),
            a.undecoded[0].form.as_deref()
        ),
        (3, "uncertain", Some("21p_undoc16"))
    );
    assert!(a.insns.iter().any(|i| i.pc == B + 4));
    assert!(!a.functions[0].complete);
}
