//! Private sections of a package part. A Rust crate has unnamed constants in sections such as `.rodata.global.13`; the package, built by the same compiler, has sections of the same names.
//! A section of the part that holds no symbol the part exports is the part's alone, so `rename` gives it a name that the package does not use, in the section list, the symbols (the section
//! symbol `.sec.<name>` too) and the relocations. A section that exports a symbol keeps its name: two owners of it are an error (`with_part`).

use crate::dko::Dko;
use std::collections::BTreeSet;

/// `part` with every section that collides with a section of `package` and exports no global symbol renamed (`<name>.p<n>`).
pub(super) fn rename(package: &Dko, part: &Dko) -> Dko {
    let mut taken: BTreeSet<String> = package
        .sections
        .iter()
        .chain(part.sections.iter())
        .map(|s| s.name.clone())
        .collect();
    let exports: BTreeSet<&str> = part
        .symbols
        .iter()
        .filter(|s| s.global)
        .filter_map(|s| s.section.as_deref())
        .collect();
    let in_package: BTreeSet<&str> = package.sections.iter().map(|s| s.name.as_str()).collect();
    let mut out = part.clone();
    for s in part
        .sections
        .iter()
        .filter(|s| in_package.contains(s.name.as_str()) && !exports.contains(s.name.as_str()))
    {
        let new = (0u32..)
            .map(|n| format!("{}.p{n}", s.name))
            .find(|c| !taken.contains(c))
            .expect("an unused name");
        taken.insert(new.clone());
        rename_section(&mut out, &s.name, &new);
    }
    out
}

/// Rename section `old` of `part` to `new` everywhere it is named.
pub(super) fn rename_section(part: &mut Dko, old: &str, new: &str) {
    let (old_sym, new_sym) = (format!(".sec.{old}"), format!(".sec.{new}"));
    for s in part.sections.iter_mut().filter(|s| s.name == old) {
        s.name = new.to_string();
    }
    for s in part.symbols.iter_mut() {
        if s.section.as_deref() == Some(old) {
            s.section = Some(new.to_string());
        }
        if s.name == old_sym {
            s.name = new_sym.clone();
        }
    }
    for r in part.relocs.iter_mut() {
        if r.section == old {
            r.section = new.to_string();
        }
        if r.symbol == old_sym {
            r.symbol = new_sym.clone();
        }
    }
}
