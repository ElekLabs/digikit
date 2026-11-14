//! Prints the analysis of a module's DSP section.
//!
//!     cargo run -p dksharc --example analyse -- MODULE.dkm DSP.dko.json [--summary]
//!
//! The first file is a `.dkm` (its DKDS section is found by the magic) or a bare DKDS section; the second is the `.dko.json` of the module's DSP
//! object. Output: the `Analysis` as JSON, or with `--summary` the counts and the functions and loops as text.

use dksharc::{Analysis, Input, analyze};

fn find_dkds(bytes: &[u8]) -> Option<&[u8]> {
    bytes
        .windows(4)
        .position(|w| w == b"DKDS")
        .map(|at| &bytes[at..])
}

fn summary(a: &Analysis) {
    let s = &a.summary;
    println!(
        "{} instructions, {} functions, {} blocks, {} loops, {} calls ({} import), {} undecoded",
        s.instructions, s.functions, s.blocks, s.loops, s.calls, s.import_calls, s.undecoded
    );
    for f in &a.functions {
        let loops = a
            .loops
            .iter()
            .filter(|l| l.start >= f.entry && l.start < f.end)
            .count();
        println!(
            "function {:<12} +{:#06x}..+{:#06x}  {} loops  complete {}",
            f.name,
            f.entry - a.base_sw,
            f.end - a.base_sw,
            loops,
            f.complete
        );
    }
    for l in &a.loops {
        println!(
            "loop depth {} body +{:#06x}..+{:#06x}  count {:?}",
            l.depth,
            l.start - a.base_sw,
            l.end - a.base_sw,
            l.count
        );
    }
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = args.iter().any(|a| a == "--summary");
    let files: Vec<&String> = args.iter().filter(|a| *a != "--summary").collect();
    let [module, dko] = files[..] else {
        return Err("usage: analyse MODULE.dkm DSP.dko.json [--summary]".into());
    };
    let read = |p: &str| std::fs::read(p).map_err(|e| format!("{p}: {e}"));
    let module = read(module)?;
    let dkds = find_dkds(&module).ok_or("no DKDS section in the module")?;
    let analysis = analyze(&Input::from_dkds(dkds, &read(dko)?)?);
    if flag {
        summary(&analysis);
    } else {
        println!(
            "{}",
            serde_json::to_string(&analysis).map_err(|e| e.to_string())?
        );
    }
    Ok(())
}
