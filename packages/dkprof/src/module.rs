//! Static analysis of a module's DSP section through `dksharc`, and its text view.

use dksharc::{Analysis, Input, analyze};
use std::fmt::Write;

/// The DKDS section of a `.dkm` (found by its magic), or the bytes themselves when they start with it.
pub fn find_dkds(bytes: &[u8]) -> Option<&[u8]> {
    bytes
        .windows(4)
        .position(|w| w == b"DKDS")
        .map(|at| &bytes[at..])
}

/// `module_analyse`: the analysis of the DSP section of `dkm` (a `.dkm` or a bare DKDS section) with the symbols of `dko_json` (its DSP `.dko.json`).
pub fn analyse(dkm: &[u8], dko_json: &[u8]) -> Result<Analysis, String> {
    let dkds = find_dkds(dkm).ok_or("no DKDS section in the module")?;
    Ok(analyze(&Input::from_dkds(dkds, dko_json)?))
}

/// Counts, functions and loops as text.
pub fn text(a: &Analysis) -> String {
    let s = &a.summary;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} instructions, {} functions, {} blocks, {} loops, {} calls ({} import), {} undecoded",
        s.instructions, s.functions, s.blocks, s.loops, s.calls, s.import_calls, s.undecoded
    );
    for f in &a.functions {
        let loops = a
            .loops
            .iter()
            .filter(|l| l.start >= f.entry && l.start < f.end)
            .count();
        let _ = writeln!(
            out,
            "function {:<12} +{:#06x}..+{:#06x}  {} loops  complete {}",
            f.name,
            f.entry - a.base_sw,
            f.end - a.base_sw,
            loops,
            f.complete
        );
    }
    for l in &a.loops {
        let _ = writeln!(
            out,
            "loop depth {} body +{:#06x}..+{:#06x}  count {:?}",
            l.depth,
            l.start - a.base_sw,
            l.end - a.base_sw,
            l.count
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DKDS: &[u8] = include_bytes!("../../dksharc/tests/fixtures/pluck-dn2.dkds");
    const DKO: &[u8] = include_bytes!("../../dksharc/tests/fixtures/pluck-dsp.dko.json");

    #[test]
    fn a_dkm_prefix_is_skipped_and_the_text_view_names_the_functions() {
        let mut dkm = b"colour of the ColdFire part".to_vec();
        dkm.extend_from_slice(DKDS);
        let a = analyse(&dkm, DKO).unwrap();
        let t = text(&a);
        assert!(t.starts_with("306 instructions, 3 functions"), "{t}");
        assert!(t.contains("function _pk_render"), "{t}");
        assert!(analyse(b"no section here", DKO).is_err());
    }
}
