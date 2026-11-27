//! `shape_index`, and the tie between `RANGE` and the wave bounds the hook assembler tests inline.
use dkrt_svc_lfo::records::{MAX_SHAPES, RANGE};
use dkrt_svc_lfo::route::shape_index;
use std::path::Path;

#[test]
fn shape_numbers_index_the_table_from_zero() {
    for (k, n) in (RANGE.lo..=RANGE.hi).enumerate() {
        assert_eq!(shape_index(n), Some(k), "number {n}");
    }
    assert_eq!(RANGE.hi - RANGE.lo + 1, MAX_SHAPES as u32);
}

#[test]
fn shape_index_is_none_outside_the_range() {
    assert_eq!(shape_index(RANGE.lo - 1), None);
    assert_eq!(shape_index(RANGE.hi + 1), None);
    assert_eq!(shape_index(u32::MAX), None);
}

/// The UI hook assembler tests the wave number inline: the name formatters the value `wave << 8` against `RANGE.lo << 8` and `RANGE.hi << 8 | 0xff`, the graph hook the wave minus `RANGE.lo`
/// against the width of `RANGE`.
#[test]
fn the_ui_hook_assembler_bounds_are_the_range() {
    let text =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("dks/src/hooks-ui.s"))
            .unwrap();
    let num = |t: &str| {
        let t = t
            .trim()
            .split(|c: char| c == ',' || c.is_whitespace())
            .next()
            .unwrap();
        match t.strip_prefix("0x") {
            Some(h) => u32::from_str_radix(h, 16).unwrap(),
            None => t.parse().unwrap(),
        }
    };
    let operands = |op: &str| -> Vec<u32> {
        text.lines()
            .filter_map(|l| l.trim_start().strip_prefix(op))
            .filter_map(|r| r.trim_start().strip_prefix('#').map(num))
            .collect()
    };
    // the name formatters (a macro, expanded twice by the file) and the graph hook
    let cmp = operands("cmpi.l");
    assert!(
        cmp.contains(&(RANGE.lo << 8)) && cmp.contains(&((RANGE.hi << 8) | 0xff)),
        "{cmp:?}"
    );
    // the graph hook: `subq.l #lo,d0; cmpi.l #(hi - lo),d0`
    assert_eq!(operands("subq.l"), [RANGE.lo]);
    assert!(cmp.contains(&(RANGE.hi - RANGE.lo)), "{cmp:?}");
}
