//! The row layouts of the registries of `core/rows` against the hand-written assembly that reads them (run by `patchkit gen --check`).
//!
//! The parameter and machine rows are arrays of 32-bit words (a number or a pointer to a string). The assembly of the parameter edits
//! (`params.s`) and of the machine hooks (`hooks-*.s`) names the offsets of fields and the row size as `.equ` constants: an edit of a
//! registry's field list that leaves one of them behind makes the firmware read the wrong word, and no build or generator notices.
use crate::error::*;
use crate::module::{Library, Module};
use std::path::Path;

/// The machine hooks of the services crate (one file per device, the same constants in each): their `.equ` constants name the offsets of the machines registry.
const MACHINE_HOOKS_DIR: &str = "services/machines/dks/src";

/// The value of `.equ NAME, value` in `text`: the first token after the comma, up to a `|`, a comment or a blank.
fn equ(text: &str, name: &str, file: &str) -> Result<u32> {
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with(&format!(".equ {name},")))
        .ok_or_else(|| err!("{file}: no `.equ {name}`"))?;
    let value = line.split(',').nth(1).unwrap_or("");
    let token = value
        .split(|c: char| c.is_whitespace() || matches!(c, '|' | ';' | '/'))
        .find(|t| !t.is_empty())
        .unwrap_or("");
    let parsed = match token.strip_prefix("0x") {
        Some(h) => u32::from_str_radix(h, 16),
        None => token.parse(),
    };
    parsed.map_err(|_| err!("{file}: `.equ {name}` has the value `{token}`, not a number"))
}

/// The byte offset of `field` in the rows of `registry`; every field must be one word.
fn offsets(m: &Module, registry: &str) -> Result<(Vec<(String, u32)>, u32)> {
    let reg = m
        .manifest
        .registry
        .iter()
        .find(|r| r.name == registry)
        .ok_or_else(|| err!("{}: no registry `{registry}`", m.id()))?;
    if let Some(f) = reg
        .field
        .iter()
        .find(|f| f.kind != "u32" && f.kind != "cstr")
    {
        return Err(err!(
            "{}: registry `{registry}` field `{}` is not one word (`{}`): the assembly reads whole words",
            m.id(),
            f.name,
            f.kind
        ));
    }
    let fields = reg
        .field
        .iter()
        .enumerate()
        .map(|(i, f)| (f.name.clone(), 4 * i as u32))
        .collect();
    Ok((fields, 4 * reg.field.len() as u32))
}

fn at(fields: &[(String, u32)], name: &str, registry: &str) -> Result<u32> {
    fields
        .iter()
        .find(|f| f.0 == name)
        .map(|f| f.1)
        .ok_or_else(|| err!("registry `{registry}` has no field `{name}`"))
}

/// The registries `params`, `param_edits` and `machines` of `core/rows` have the offsets that `params.s` and the machine hooks name.
pub fn check(lib: &Library, root: &Path) -> Result<()> {
    let Some(rows) = lib.get("core/rows") else {
        return Ok(());
    };
    // the one `params.s` under src/cf: both devices link it
    let params = std::fs::read_dir(rows.dir.join("src/cf"))
        .map_err(|e| err!("{}: {e}", rows.dir.join("src/cf").display()))?
        .filter_map(|d| d.ok())
        .map(|d| d.path().join("params.s"))
        .find(|p| p.exists())
        .ok_or_else(|| err!("core/rows: no src/cf/*/params.s"))?;
    let text = std::fs::read_to_string(&params).map_err(|e| err!("{}: {e}", params.display()))?;
    let file = params.display().to_string();
    let (params_fields, _) = offsets(rows, "params")?;
    let (edit_fields, _) = offsets(rows, "param_edits")?;
    for (name, want, registry, field) in [
        ("P_TEMPLATE", &params_fields, "params", "template"),
        ("E_CLASS", &edit_fields, "param_edits", "class"),
    ] {
        let offset = at(want, field, registry)?;
        let have = equ(&text, name, &file)?;
        if have != offset {
            return Err(err!(
                "{file}: `.equ {name}` is {have}, but field `{field}` of registry `{registry}` is at byte {offset}"
            ));
        }
    }
    let (machine_fields, size) = offsets(rows, "machines")?;
    let dir = root.join(MACHINE_HOOKS_DIR);
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .map_err(|e| err!("{}: {e}", dir.display()))?
        .filter_map(|d| d.ok())
        .map(|d| d.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("hooks-") && n.ends_with(".s"))
        })
        .collect();
    files.sort();
    // the file that names the row layout (`hooks-common.s` and the per-device files may not)
    let (hooks_file, hooks) = files
        .iter()
        .filter_map(|p| Some((p.display().to_string(), std::fs::read_to_string(p).ok()?)))
        .find(|(_, text)| text.contains(".equ ROWSIZE,"))
        .ok_or_else(|| err!("{}: no hooks-*.s with `.equ ROWSIZE`", dir.display()))?;
    for (name, want) in [
        ("O_CLONE", at(&machine_fields, "clone_of", "machines")?),
        ("O_OWN", at(&machine_fields, "own_dsp", "machines")?),
        ("ROWSIZE", size),
    ] {
        let have = equ(&hooks, name, &hooks_file)?;
        if have != want {
            return Err(err!(
                "{hooks_file}: `.equ {name}` is {have}, but the machines registry has {want}"
            ));
        }
    }
    Ok(())
}
