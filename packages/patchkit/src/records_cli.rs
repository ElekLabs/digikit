//! `patchkit records --json FILE|-`: the records of a module's committed record block, decoded.
//!
//! `patchkit records --check FILE|-` runs the rule of each record kind's owning service over the block (`record_rules`) and fails with the problems.
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

/// The records of the block BYTES grouped by kind, each kind's bodies in file order. The block's layout is the register table's (header of 16 bytes, then kind, length, body).
#[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
fn bodies_by_kind(bytes: &[u8]) -> Result<Vec<(u32, Vec<&[u8]>)>> {
    let word = |at: usize| -> Result<u32> {
        bytes
            .get(at..at + 4)
            .map(|b| u32::from_be_bytes(b.try_into().unwrap()))
            .ok_or_else(|| Error("record block: read past the end".into()))
    };
    if word(0)? != crate::register::MAGIC || word(4)? != crate::register::VERSION {
        return Err(Error("record block: magic or version".into()));
    }
    let mut groups: Vec<(u32, Vec<&[u8]>)> = vec![];
    let mut at = crate::register::HEADER;
    for _ in 0..word(12)? {
        let (kind, len) = (word(at)?, word(at + 4)? as usize);
        let body = bytes
            .get(at + 8..at + len)
            .filter(|_| len >= 8)
            .ok_or_else(|| Error("record block: a record runs past the end".into()))?;
        match groups.iter_mut().find(|g| g.0 == kind) {
            Some(g) => g.1.push(body),
            None => groups.push((kind, vec![body])),
        }
        at += len;
    }
    Ok(groups)
}

/// The problems the owning services find in the block BYTES, one sentence each (empty when the block is good). Needs the feature `record-rules` (not for wasm32).
#[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
pub fn block_problems(bytes: &[u8]) -> Result<Vec<String>> {
    let mut out = vec![];
    for (kind, bodies) in bodies_by_kind(bytes)? {
        let name = dkneeds::KIND_NAMES
            .iter()
            .find(|k| k.1 == kind)
            .map_or("?", |k| k.0);
        for p in crate::record_rules::validate(kind, &bodies) {
            out.push(format!("{name}: {p}"));
        }
    }
    Ok(out)
}

#[cfg(not(all(feature = "record-rules", not(target_arch = "wasm32"))))]
pub fn block_problems(_bytes: &[u8]) -> Result<Vec<String>> {
    Err(Error(
        "records --check is not available: patchkit is not built with record rules (feature record-rules; not for wasm32)".into(),
    ))
}

/// `problems` of `records --json`: the list, or `null` where the check is not available (no record rules built in), so that the decoded rows stay available there.
#[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
fn problems_json(bytes: &[u8]) -> Result<Value> {
    Ok(json!(block_problems(bytes)?))
}

#[cfg(not(all(feature = "record-rules", not(target_arch = "wasm32"))))]
fn problems_json(_bytes: &[u8]) -> Result<Value> {
    Ok(Value::Null)
}

/// The decoded records of the block BYTES as JSON: `counts` (the three `[uses]` numbers) and the rows.
pub fn records_json(bytes: &[u8]) -> Result<Value> {
    let (r, filter_param_rows) = Register::parse_with_filter_param_rows(bytes)?;
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
        "counts": {"machine_rows": machines.len(), "param_rows": params.len() + filter_param_rows, "page_rows": pages.len()},
        "problems": problems_json(bytes)?,
        "device": r.device,
        "machines": machines, "params": params, "pages": pages, "effects": effects,
    }))
}

/// The subcommand: arguments after `records`.
pub fn run(rest: &[String]) -> Result<()> {
    let files: Vec<&String> = rest.iter().filter(|a| !a.starts_with("--")).collect();
    let check = rest.iter().any(|a| a == "--check");
    let [f] = files.as_slice() else {
        return Err(Error(
            "usage: patchkit records --json FILE|-  or  patchkit records --check FILE|-".into(),
        ));
    };
    let raw = if f.as_str() == "-" {
        let mut b = vec![];
        std::io::Read::read_to_end(&mut std::io::stdin(), &mut b)
            .map_err(|e| Error(format!("stdin: {e}")))?;
        b
    } else {
        std::fs::read(f).map_err(|e| Error(format!("{f}: {e}")))?
    };
    if check {
        let problems = block_problems(&raw)?;
        if !problems.is_empty() {
            return Err(Error(format!(
                "the records are refused:\n  {}",
                problems.join("\n  ")
            )));
        }
        println!("records ok");
        return Ok(());
    }
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

    #[test]
    fn a_filters_own_knobs_count_as_parameter_rows() {
        let mut block = vec![];
        for word in [
            crate::register::MAGIC,
            crate::register::VERSION,
            1,
            1,
            dkneeds::KIND_FILTER,
            dkneeds::LEN_FILTER,
            0x1234_5600,
            2,
        ] {
            block.extend_from_slice(&word.to_be_bytes());
        }
        block.extend_from_slice(b"Dual Band");
        block.resize(crate::register::HEADER + 16 + 24, 0);
        block.extend_from_slice(b"DUAL");
        block.resize(crate::register::HEADER + 16 + 48, 0);
        for (name, short, default) in [("Spread", "SPRD", 64u32), ("Resonance", "RES", 20)] {
            let start = block.len();
            for word in [0, 0, 127 * 256, default * 256] {
                block.extend_from_slice(&word.to_be_bytes());
            }
            block.extend_from_slice(name.as_bytes());
            block.resize(start + 48, 0);
            block.extend_from_slice(short.as_bytes());
            block.resize(start + 64, 0);
        }

        let decoded = records_json(&block).unwrap();
        assert_eq!(
            decoded["counts"],
            json!({"machine_rows": 0, "param_rows": 2, "page_rows": 0})
        );
        assert_eq!(decoded["params"], json!([]));
        #[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
        assert_eq!(decoded["problems"], json!([]));
        assert!(records_json(&block[..block.len() - 1]).is_err());
    }

    #[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
    fn lfo_block(shapes: &[(u32, &[u8; 4], i16)]) -> Vec<u8> {
        let mut o = vec![];
        for v in [
            crate::register::MAGIC,
            crate::register::VERSION,
            1,
            shapes.len() as u32,
        ] {
            o.extend_from_slice(&v.to_be_bytes());
        }
        for (key, name, from) in shapes {
            o.extend_from_slice(&dkneeds::KIND_LFO_SHAPE.to_be_bytes());
            o.extend_from_slice(&dkneeds::LEN_LFO_SHAPE.to_be_bytes());
            o.extend_from_slice(&key.to_be_bytes());
            o.extend_from_slice(*name);
            for i in 0..32i16 {
                o.extend_from_slice(&from.wrapping_add(2048i16.wrapping_mul(i)).to_be_bytes());
            }
        }
        o
    }

    #[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
    #[cfg(all(feature = "record-rules", not(target_arch = "wasm32")))]
    #[test]
    fn a_block_of_lfo_shapes_decodes_and_is_judged_by_the_lfo_service() {
        let ok = lfo_block(&[(0x100, b"RAMP", -31744)]);
        assert!(
            records_json(&ok).unwrap()["problems"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(block_problems(&ok).unwrap().is_empty());

        // names that share their first three characters
        let bad = lfo_block(&[(0x100, b"RAMP", -31744), (0x200, b"RAMQ", -31744)]);
        let problems = block_problems(&bad).unwrap();
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(
            problems[0].starts_with("lfo_shape: record kind 8, record 1:"),
            "{problems:?}"
        );
        assert_eq!(
            records_json(&bad).unwrap()["problems"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        // a block with a record that runs past the end
        assert!(block_problems(&bad[..bad.len() - 4]).is_err());
    }
}
