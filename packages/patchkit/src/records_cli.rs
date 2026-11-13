//! `patchkit records --json FILE|-`: the records of a module's committed record block, decoded.
//!
//! The block is in the format of the register table (`register::Register::parse`). `module:check` and conformance read it from the
//! emulated device (the runtime's `MODULE_COMMIT` event gives its address and size) and take the counts for `[uses]`, the parameter
//! rows (slot, min, max, default, names) and the pages from this output, so no second decoder exists.

use crate::error::*;
use crate::register::Register;
use serde_json::{Value, json};

fn f32_of(bits: u32) -> f64 {
    f32::from_bits(bits) as f64
}

/// The decoded records of the block BYTES as JSON: `counts` (the three `[uses]` numbers) and the rows.
pub fn records_json(bytes: &[u8]) -> Result<Value> {
    let r = Register::parse(bytes)?;
    let params: Vec<Value> = r
        .params
        .iter()
        .map(|p| {
            json!({"index": p.index, "template": p.template, "slot": p.slot, "min": p.min, "max": p.max,
                   "default": p.default, "name": p.name, "short": p.short})
        })
        .collect();
    let machines: Vec<Value> = r
        .machines
        .iter()
        .map(|m| {
            json!({"type": m.type_no, "clone_of": m.clone_of, "position": m.position, "own_dsp": m.own_dsp,
                   "level": m.level, "knobs": m.knobs, "name": m.name, "short": m.short})
        })
        .collect();
    let pages: Vec<Value> = r
        .pages
        .iter()
        .map(|p| json!({"type": p.type_no, "knobs": p.knobs, "level": p.level, "name": p.name, "title": p.title}))
        .collect();
    let effects: Vec<Value> = r
        .effects
        .iter()
        .map(|e| {
            let knobs: Vec<Value> = e
                .knobs
                .iter()
                .map(|k| {
                    json!({"position": k.position, "display": k.display, "min": f32_of(k.min),
                           "max": f32_of(k.max), "default": f32_of(k.default), "name": k.name, "short": k.short})
                })
                .collect();
            json!({"fx": e.fx, "id": e.id, "mix": e.mix, "tab": e.tab, "title": e.title, "knobs": knobs})
        })
        .collect();
    Ok(json!({
        "counts": {"machine_rows": machines.len(), "param_rows": params.len(), "page_rows": pages.len()},
        "device": r.device,
        "machines": machines, "params": params, "pages": pages, "effects": effects,
    }))
}

/// The subcommand: arguments after `records`.
pub fn run(rest: &[String]) -> Result<()> {
    let files: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--")).collect();
    let [f] = files.as_slice() else {
        return Err(Error("usage: patchkit records --json FILE|-".into()));
    };
    let raw = if f.as_str() == "-" {
        let mut b = vec![];
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut b)
            .map_err(|e| Error(format!("stdin: {e}")))?;
        b
    } else {
        std::fs::read(f).map_err(|e| Error(format!("{f}: {e}")))?
    };
    println!("{}", serde_json::to_string_pretty(&records_json(&raw)?)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::register::{Machine, Param, Register};

    #[test]
    fn counts_and_rows_come_from_the_register_format() {
        let reg = Register {
            device: 1,
            params: vec![Param {
                index: 0,
                template: 0,
                slot: 26,
                min: 0,
                max: 100,
                default: 50,
                name: "Cutoff".into(),
                short: "CUT".into(),
            }],
            machines: vec![Machine {
                type_no: 0x1234_5600,
                clone_of: 0,
                position: 0,
                own_dsp: 1,
                level: 0,
                knobs: [0; crate::register::KNOBS],
                name: "Analog".into(),
                short: "ANLG".into(),
            }],
            ..Default::default()
        };
        let v = records_json(&reg.encode()).unwrap();
        assert_eq!(v["counts"]["machine_rows"], 1);
        assert_eq!(v["counts"]["param_rows"], 1);
        assert_eq!(v["counts"]["page_rows"], 0);
        assert_eq!(v["params"][0]["name"], "Cutoff");
        assert_eq!(v["params"][0]["max"], 100);
        assert!(records_json(&[0u8; 8]).is_err());
    }
}
