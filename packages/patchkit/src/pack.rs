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
}

impl Packed {
    /// The address map of the code the loader places (empty for a data-only module).
    pub fn addrmap(&self) -> Result<dkaddrmap::AddrMap> {
        Ok(match &self.cf {
            Some(o) => crate::addrmap::module_map(o, &self.name, &self.dkm.sha256)?,
            None => dkaddrmap::AddrMap::new(
                "module",
                dkaddrmap::Key {
                    module: Some(self.name.clone()),
                    dkm_sha256: Some(self.dkm.sha256.clone()),
                    ..Default::default()
                },
                vec![],
            ),
        })
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
    };
    // a run-time module declares in `init`: `[[machine]]`/`[[fx]]` in module.toml; the table at 0x90 is the keys table
    let mut dsp: Vec<u8> = vec![];
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
        opts.stereo = s.stereo;
        opts.gate = s.gate;
        dsp = s.dsp;
        s.keys
    } else {
        vec![]
    };
    let bytes = crate::dkm::pack_keys(cf.first().map(|c| c.0), &reg, &dsp, &opts)?;
    let dkm = crate::dkm::Dkm::parse(&bytes)?;
    Ok(Packed {
        bytes,
        dkm,
        name: opts.name,
        cf: cf.first().map(|c| c.0.clone()),
    })
}
