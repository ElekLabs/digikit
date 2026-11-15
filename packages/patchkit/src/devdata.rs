//! The devices patchkit knows, from the device files (`devices/*.toml`, embedded by `build.rs`): short name, the device byte of
//! the firmware SysEx framing, the id and name digikit uses, and the container limits of a port. Nothing in the code names a
//! device: a new device file adds one (after a rebuild). The seal keys are not device-profile data; see `syx.rs`.

use crate::error::*;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

include!(concat!(env!("OUT_DIR"), "/device_files.rs"));

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub short: String,
    pub name: String,
    /// The device byte of the firmware SysEx framing.
    pub syx_id: u8,
    /// What digikit calls the device (runtime, registry, DSP table, `.dkm`) and the name its INFO reply gives.
    pub digikit_id: u32,
    pub digikit_name: String,
    /// `[patch]`: container and section limits of a port (a default for an image with no port).
    pub container_max: u64,
    pub dsp_stored_max: u64,
    pub dsp_depacked_max: u64,
    pub main_staged_max: u64,
    /// `[patch] stream_id`: the id byte of the OS UPGRADE header message (0x0f DT2, 0x10 DN2).
    pub stream_id: u8,
    /// `[patch] build_floor`: the four ASCII characters the OS validator compares the container's build word with (DT2 "0060");
    /// `None` when the OS has no floor (DN2).
    pub build_floor: Option<[u8; 4]>,
    /// `[limits] modules`: module slots of the runtime's tables.
    pub modules: Option<u64>,
    /// `[limits] fx_slots`: effects the device's chain takes (at most the contract's `DK_FX_MAX`).
    pub fx_slots: Option<u64>,
    /// `[patch_sets]`: named module sets as roots (the modules they require are added), e.g. `emulator` and `device`.
    pub patch_sets: BTreeMap<String, Vec<String>>,
    /// `[board] dsp_clock_hz` and `dsp_blocks_per_second`: the DSP block length the budget starts from.
    pub dsp_clock_hz: Option<u64>,
    pub dsp_blocks_per_second: Option<u64>,
    /// `[adapter.machine] voices`: voices of the machine adapter.
    pub machine_voices: Option<u64>,
}

impl DeviceInfo {
    /// The roots of a named set (`device`, `emulator`).
    pub fn patch_set(&self, name: &str) -> Result<&[String]> {
        self.patch_sets.get(name).map(Vec::as_slice).ok_or_else(|| {
            let known: Vec<&str> = self.patch_sets.keys().map(String::as_str).collect();
            err!(
                "{} has no patch set {name:?} (known: {})",
                self.short,
                known.join(", ")
            )
        })
    }

    /// A device limit by the name a registry's `limit` key uses; `None` for an unknown name or a device file without it.
    pub fn limit(&self, name: &str) -> Option<u64> {
        match name {
            "fx_slots" => self.fx_slots,
            _ => None,
        }
    }
}

/// Names a registry's `limit` may use.
pub const LIMIT_NAMES: [&str; 1] = ["fx_slots"];

#[derive(Deserialize)]
struct File {
    device: Option<Dev>,
    digikit: Option<Digikit>,
    patch: Option<Patch>,
    limits: Option<LimitsDoc>,
    board: Option<BoardDoc>,
    adapter: Option<AdapterDoc>,
    #[serde(default)]
    patch_sets: BTreeMap<String, Vec<String>>,
}
#[derive(Deserialize)]
struct BoardDoc {
    dsp_clock_hz: Option<u64>,
    dsp_blocks_per_second: Option<u64>,
}
#[derive(Deserialize)]
struct AdapterDoc {
    machine: Option<MachineDoc>,
}
#[derive(Deserialize)]
struct MachineDoc {
    voices: Option<u64>,
}
#[derive(Deserialize)]
struct LimitsDoc {
    modules: Option<u64>,
    fx_slots: Option<u64>,
}
#[derive(Deserialize)]
struct Dev {
    name: String,
    short: String,
    syx_id: Option<u8>,
}
#[derive(Deserialize)]
struct Digikit {
    id: u32,
    name: String,
}
#[derive(Deserialize)]
struct Patch {
    container_max: u64,
    dsp_stored_max: u64,
    dsp_depacked_max: u64,
    main_staged_max: u64,
    stream_id: u8,
    build_floor: Option<String>,
}

fn parse(files: &[(&str, &str)]) -> Result<Vec<DeviceInfo>> {
    let mut out: Vec<DeviceInfo> = vec![];
    for (n, text) in files {
        let f: File = toml::from_str(text).map_err(|e| err!("devices/{n}: {e}"))?;
        let Some(d) = f.device else { continue };
        let (Some(syx_id), Some(k), Some(p)) = (d.syx_id, f.digikit, f.patch) else {
            return Err(err!(
                "devices/{n}: [device] syx_id, [digikit] and [patch] are required"
            ));
        };
        if out.iter().any(|o| o.short == d.short) {
            return Err(err!("devices/{n}: device {} is described twice", d.short));
        }
        if out.iter().any(|o| o.syx_id == syx_id) {
            return Err(err!("devices/{n}: syx_id {syx_id:#04x} is used twice"));
        }
        if out.iter().any(|o| o.digikit_id == k.id) {
            return Err(err!("devices/{n}: [digikit] id {} is used twice", k.id));
        }
        out.push(DeviceInfo {
            short: d.short,
            name: d.name,
            syx_id,
            digikit_id: k.id,
            digikit_name: k.name,
            container_max: p.container_max,
            dsp_stored_max: p.dsp_stored_max,
            dsp_depacked_max: p.dsp_depacked_max,
            main_staged_max: p.main_staged_max,
            stream_id: p.stream_id,
            build_floor: match p.build_floor {
                None => None,
                Some(s) => {
                    Some(<[u8; 4]>::try_from(s.as_bytes()).map_err(|_| {
                        err!("devices/{n}: [patch] build_floor must be 4 characters")
                    })?)
                }
            },
            modules: f.limits.as_ref().and_then(|l| l.modules),
            fx_slots: f.limits.and_then(|l| l.fx_slots),
            patch_sets: f.patch_sets,
            dsp_clock_hz: f.board.as_ref().and_then(|b| b.dsp_clock_hz),
            dsp_blocks_per_second: f.board.as_ref().and_then(|b| b.dsp_blocks_per_second),
            machine_voices: f.adapter.and_then(|a| a.machine).and_then(|m| m.voices),
        });
    }
    Ok(out)
}

/// Every device file's device, in file-name order.
pub fn all() -> &'static [DeviceInfo] {
    static ALL: OnceLock<Vec<DeviceInfo>> = OnceLock::new();
    ALL.get_or_init(|| parse(DEVICE_FILES).unwrap_or_else(|e| panic!("embedded device files: {e}")))
}

pub fn by_short(short: &str) -> Option<&'static DeviceInfo> {
    all().iter().find(|d| d.short == short)
}

pub fn by_syx_id(id: u8) -> Option<&'static DeviceInfo> {
    all().iter().find(|d| d.syx_id == id)
}

/// `dt2|dn2` for a usage text or an error.
pub fn short_list() -> String {
    all()
        .iter()
        .map(|d| d.short.as_str())
        .collect::<Vec<_>>()
        .join("|")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_embedded_device_files_parse() {
        let d = all();
        assert!(d.len() >= 2);
        let dt2 = by_short("dt2").unwrap();
        assert_eq!(
            (dt2.syx_id, dt2.digikit_id, dt2.digikit_name.as_str()),
            (0x14, 1, "dt2")
        );
        assert_eq!(by_syx_id(0x15).unwrap().short, "dn2");
        assert!(by_short("nope").is_none());
    }

    #[test]
    fn a_device_without_its_data_is_refused() {
        let bad = [("x.toml", "[device]\nname = \"X\"\nshort = \"x\"\n")];
        assert!(parse(&bad).is_err());
        let dup = [
            ("a.toml", include_str!("../../../devices/digitakt-ii.toml")),
            ("b.toml", include_str!("../../../devices/digitakt-ii.toml")),
        ];
        assert!(parse(&dup).is_err());
    }
}
