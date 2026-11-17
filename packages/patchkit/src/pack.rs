//! Pack one run-time module as a .dkm: the build of its stale objects, the CF object, and for a run-time (dynamic) module its DSP sections for one device. The CLI `pack` and the desktop app's module library call it.

use crate::error::*;
use crate::module::{Library, Module, ModuleKind};

/// What `pack_module` takes besides the library and the module id.
#[derive(Debug, Default, Clone)]
pub struct PackOptions {
    /// `--device`: required for a run-time (dynamic) module.
    pub device: Option<String>,
    /// `--name`: the name in the .dkm (default: the module id).
    pub name: Option<String>,
    /// `--trace`: sets the trace bit.
    pub trace: bool,
}

/// A packed module.
pub struct Packed {
    /// The .dkm file.
    pub bytes: Vec<u8>,
    /// The same file, parsed.
    pub dkm: crate::dkm::Dkm,
    name: String,
    cf: Option<crate::dko::Dko>,
    dsp: Option<crate::dko::Dko>,
}

impl Packed {
    /// The address map of the code the loader places: ColdFire symbols, and for a module with a DSP object its `sharc` symbols (empty for a data-only module).
    pub fn addrmap(&self) -> Result<dkaddrmap::AddrMap> {
        crate::addrmap::module_map(
            self.cf.as_ref(),
            self.dsp.as_ref(),
            &self.name,
            &self.dkm.sha256,
        )
    }
}

/// A raw, non-dynamic module whose ColdFire object defines no `dk_module`: it can only be patched into an image (patchkit
/// fixtures such as test/cf-demo), never loaded as a `.dkm`.
pub fn patch_only(m: &Module) -> bool {
    !m.manifest.module.dynamic
        && matches!(m.manifest.module.kind, ModuleKind::Raw)
        && m.objects.iter().find(|o| o.arch == "cf").is_some_and(|o| {
            !o.symbols
                .iter()
                .any(|s| s.name == "dk_module" && s.section.is_some())
        })
}

/// Pack the modules `ids` for `device` in this process: one library for all of them (objects built when stale), or, when that load
/// fails (a module's build failed), one library per module so the failure is that module's and the others still pack. Patch-only
/// modules (`patch_only`) are left out of the result.
#[cfg(not(target_arch = "wasm32"))]
pub fn pack_all(
    root: &std::path::Path,
    ids: &[String],
    device: &str,
) -> Vec<(String, std::result::Result<Packed, String>)> {
    let source = crate::module::LibrarySource::default();
    let opts = PackOptions {
        device: Some(device.into()),
        ..Default::default()
    };
    let pack_in = |lib: &Library, id: &String| {
        if lib.get(id).is_some_and(patch_only) {
            return None;
        }
        Some((
            id.clone(),
            pack_module(lib, id, &opts).map_err(|e| e.to_string()),
        ))
    };
    match crate::module::selected_library(root, &source, false, ids, ids) {
        Ok(lib) => ids.iter().filter_map(|id| pack_in(&lib, id)).collect(),
        Err(_) => ids
            .iter()
            .filter_map(|id| {
                match crate::module::selected_library(
                    root,
                    &source,
                    false,
                    std::slice::from_ref(id),
                    std::slice::from_ref(id),
                ) {
                    Ok(lib) => pack_in(&lib, id),
                    Err(e) => Some((id.clone(), Err(e.to_string()))),
                }
            })
            .collect(),
    }
}

/// The object of a service with its firmware references renamed to the import names `patchkit bind` resolves. An undefined symbol that has an entry in
/// `externs` takes the name of its target (`cf.<name>`, `hook.<hook>.tail|stock|ret|exit|exit.<n>`); an undefined symbol named like a firmware name (`cf.*`: what the
/// `dkfw` crate links) takes `dk_fw_<hash>`. Every other undefined symbol stays an import of the runtime.
pub fn firmware_imports(
    obj: &crate::dko::Dko,
    externs: &std::collections::BTreeMap<String, String>,
) -> Result<crate::dko::Dko> {
    // the ISA record covers the symbol names: check the object as built, then stamp the renamed one
    obj.require_isa_checked()?;
    let defined: std::collections::BTreeSet<&str> = obj
        .symbols
        .iter()
        .filter(|s| s.section.is_some())
        .map(|s| s.name.as_str())
        .collect();
    let map = |n: &str| -> Result<Option<String>> {
        if defined.contains(n) {
            return Ok(None);
        }
        match externs.get(n) {
            Some(t) => crate::bind::import_for_target(t).map(Some).ok_or_else(|| {
                Error(format!(
                    "extern {n} = {t:?}: a service names `cf.<name>` or `hook.<hook>.tail|stock|ret|exit|exit.<n>`"
                ))
            }),
            None if n.starts_with("cf.") => Ok(Some(crate::bind::fw_import(n))),
            None => Ok(None),
        }
    };
    let mut out = obj.clone();
    let mut seen = std::collections::BTreeSet::new();
    let mut symbols = vec![];
    for s in &obj.symbols {
        let mut s = s.clone();
        if s.section.is_none()
            && let Some(n) = map(&s.name)?
        {
            s.name = n;
        }
        if s.section.is_some() || seen.insert(s.name.clone()) {
            symbols.push(s);
        }
    }
    out.symbols = symbols;
    for r in &mut out.relocs {
        if let Some(n) = map(&r.symbol)? {
            r.symbol = n;
        }
    }
    out.stamp_isa();
    Ok(out)
}

/// Packs the objects of module `id` as they are in memory in `lib`.
pub fn pack_module(lib: &Library, id: &str, o: &PackOptions) -> Result<Packed> {
    let m = lib
        .get(id)
        .ok_or_else(|| Error(format!("no module {id}")))?;
    let cf: Vec<_> = m
        .objects
        .iter()
        .zip(&m.manifest.object)
        .filter(|(o, _)| o.arch == "cf")
        .collect();
    let dynamic = m.manifest.module.dynamic;
    if cf.len() > 1 || (cf.is_empty() && !dynamic) {
        return Err(Error(format!(
            "{id} has {} ColdFire objects; a .dkm packs exactly one",
            cf.len()
        )));
    }
    let mut opts = crate::dkm::Options {
        name: o.name.clone().unwrap_or_else(|| id.to_string()),
        version: crate::dkm::parse_version(&m.manifest.module.version)?,
        trace: o.trace,
        stereo: false,
        gate: false,
        service: crate::module::service_of(m)?,
    };
    // a fill for a slot that only some devices have is left out of the file of the others
    if let Some(dev) = &o.device {
        opts.service = opts.service.take().map(|s| s.for_device(dev));
    }
    // a run-time module declares in `init`: `[[machine]]`/`[[fx]]` in module.toml; the table at 0x90 is the keys table
    let mut dsp: Vec<u8> = vec![];
    let mut dsp_obj = None;
    let reg = if dynamic {
        let dev = o.device.clone().ok_or_else(|| {
            Error(format!(
                "a run-time module needs --device {}",
                crate::devdata::short_list()
            ))
        })?;
        let ports = &m.manifest.module.ports;
        if !m.manifest.module.builds_for(&dev) {
            return Err(Error(format!(
                "{id} is not built for {dev}: its ports are {}",
                ports.join(", ")
            )));
        }
        let sharc: Vec<_> = m
            .objects
            .iter()
            .zip(&m.manifest.object)
            .filter(|(o, r)| {
                o.arch == "sharc" && (r.devices.is_empty() || r.devices.iter().any(|d| *d == dev))
            })
            .collect();
        if sharc.len() > 1 {
            return Err(Error(format!(
                "{id} has {} DSP objects for {dev}; a .dkm packs one",
                sharc.len()
            )));
        }
        let s = crate::major3::sections(m, &opts.name, &dev, sharc.first().map(|(o, _)| *o))?;
        dsp_obj = sharc.first().map(|(o, _)| (*o).clone());
        opts.stereo = s.stereo;
        opts.gate = s.gate;
        dsp = s.dsp;
        s.keys
    } else {
        vec![]
    };
    // a service file names firmware through import names that `patchkit bind` resolves (`bind::import_for_target`)
    let service_obj = match cf.first() {
        Some((dko, r)) if m.manifest.module.role == Some(crate::module::ModuleRole::Service) => {
            Some(firmware_imports(dko, &r.externs)?)
        }
        _ => None,
    };
    let bytes = crate::dkm::pack_keys(
        service_obj.as_ref().or(cf.first().map(|c| c.0)),
        &reg,
        &dsp,
        &opts,
    )?;
    let dkm = crate::dkm::Dkm::parse(&bytes)?;
    Ok(Packed {
        bytes,
        dkm,
        name: opts.name,
        cf: service_obj.or(cf.first().map(|c| c.0.clone())),
        dsp: dsp_obj,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dko::{Dko, FORMAT};

    const HEAD: &str = "[module]\nid = \"test/pk\"\nversion = \"0.1.0\"\nname = \"PK\"\ncategory = \"test\"\ntype = \"raw\"\nauthor = \"digikit\"\nstatus = \"experimental\"\nsummary = \"s\"\n";

    /// An object of `arch` with no code: `pack_module` looks at the architecture before it reads anything else.
    fn dko(arch: &str) -> String {
        let o = Dko {
            format: FORMAT,
            arch: arch.into(),
            source_sha256: "0".repeat(64),
            toolchain: Default::default(),
            sections: vec![],
            symbols: vec![],
            relocs: vec![],
        };
        serde_json::to_string(&o).unwrap()
    }

    /// A library of one module `test/pk` with the `[module]` keys `keys` and the objects `(arch, path)` in this order.
    fn library(keys: &str, objects: &[(&str, &str)]) -> Library {
        let mut text = format!("{HEAD}{keys}");
        for (_, path) in objects {
            text.push_str(&format!(
                "[[object]]\npath = \"{path}\"\nplace = \"cf.expansion\"\n"
            ));
        }
        let arch_of = |path: &str| {
            objects
                .iter()
                .find(|(_, p)| *p == path)
                .map(|(a, _)| *a)
                .unwrap()
        };
        let m = Module::from_parts(&text, &|p| Ok(dko(arch_of(p)))).unwrap();
        Library { modules: vec![m] }
    }

    fn refused(lib: &Library, device: Option<&str>) -> String {
        let o = PackOptions {
            device: device.map(String::from),
            ..Default::default()
        };
        match pack_module(lib, "test/pk", &o) {
            Ok(_) => panic!("the module must be refused"),
            Err(e) => e.0,
        }
    }

    #[test]
    fn a_module_with_two_coldfire_objects_or_none_is_refused() {
        let lib = library("", &[("cf", "a.dko.json"), ("cf", "b.dko.json")]);
        assert!(refused(&lib, None).contains("2 ColdFire objects"));
        // a module that is not dynamic and has none is refused the same way
        let lib = library("", &[("sharc", "d.dko.json")]);
        assert!(refused(&lib, None).contains("0 ColdFire objects"));
    }

    #[test]
    fn a_run_time_module_needs_a_device_that_its_ports_include() {
        let keys = "dynamic = true\nports = [\"dt2-1.17\"]\n";
        let lib = library(keys, &[("cf", "a.dko.json")]);
        assert!(refused(&lib, None).contains("needs --device"));
        let e = refused(&lib, Some("dn2"));
        assert!(
            e.contains("is not built for dn2") && e.contains("dt2-1.17"),
            "{e}"
        );
    }

    #[test]
    fn a_run_time_module_with_two_dsp_objects_for_the_device_is_refused() {
        let keys = "dynamic = true\n";
        let lib = library(
            keys,
            &[
                ("cf", "a.dko.json"),
                ("sharc", "d1.dko.json"),
                ("sharc", "d2.dko.json"),
            ],
        );
        assert!(refused(&lib, Some("dt2")).contains("2 DSP objects for dt2"));
    }

    #[test]
    fn an_unknown_module_id_is_an_error_of_pack_module_and_an_error_entry_of_pack_all() {
        let e = match pack_module(&Library::default(), "test/nope", &PackOptions::default()) {
            Ok(_) => panic!("no such module"),
            Err(e) => e.0,
        };
        assert!(e.contains("no module test/nope"), "{e}");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let packed = pack_all(&root, &["test/nope".to_string()], "dt2");
        assert_eq!(packed.len(), 1);
        assert_eq!(packed[0].0, "test/nope");
        assert!(packed[0].1.is_err());
    }
}
