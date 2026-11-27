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

/// Every `cmpi.l #N,Dn` of a hook file is one of the two bounds, each hook has the pair, and the bounds are `RANGE`'s.
#[test]
fn the_hook_assembler_bounds_are_the_range() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("dks/src");
    let mut devs: Vec<String> = std::fs::read_dir(&src)
        .unwrap()
        .filter_map(|e| {
            let name = e.ok()?.file_name().into_string().ok()?;
            let id = name.strip_prefix("hooks-")?.strip_suffix(".s")?;
            (id != "ui").then(|| id.to_string())
        })
        .collect();
    devs.sort();
    assert!(!devs.is_empty(), "no hooks-<device>.s in {}", src.display());
    for dev in devs {
        let path = src.join(format!("hooks-{dev}.s"));
        let text = std::fs::read_to_string(&path).unwrap();
        let bounds: Vec<u32> = text
            .lines()
            .filter_map(|l| l.trim_start().strip_prefix("cmpi.l"))
            .filter_map(|r| {
                r.trim_start()
                    .strip_prefix('#')?
                    .split(',')
                    .next()?
                    .parse()
                    .ok()
            })
            .collect();
        // six hooks, each tests the low bound then the high bound
        assert_eq!(bounds.len(), 12, "{dev}: {bounds:?}");
        for pair in bounds.chunks(2) {
            assert_eq!(pair, [RANGE.lo, RANGE.hi], "{dev}");
        }
    }
}
