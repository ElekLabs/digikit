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
    /// What digikit calls the device (runtime, registry, DSP table, `.dkm`) and the name its INFO reply gives. `None`: the device
    /// file has no `[digikit]` table, so the device is not a runtime target.
    pub digikit_id: Option<u32>,
    pub digikit_name: Option<String>,
    /// `[capabilities]`, `[container]` and `[acceptance]` (see [`Identity`]).
    pub identity: Identity,
    /// `[patch]`: container and section limits of a port (a default for an image with no port). The `dsp_*` limits are 0 on a
    /// device without a DSP (`[capabilities] dsp = "none"`).
    pub container_max: u64,
    pub dsp_stored_max: u64,
    pub dsp_depacked_max: u64,
    pub main_staged_max: u64,
    /// `[patch] stream_id`: the id byte of the OS UPGRADE header message (0x0f DT2, 0x10 DN2).
    pub stream_id: u8,
    /// `[patch] build_floor`: the four ASCII characters the OS validator compares the container's build word with (DT2 "0060");
    /// `None` when the OS has no floor (DN2).
    pub build_floor: Option<[u8; 4]>,
    /// `[limits] fx_slots`: effects the device's chain takes (at most the contract's `DK_FX_MAX`).
    pub fx_slots: Option<u64>,
    /// `[patch_sets]`: named module sets as roots (the modules they require are added), e.g. `emulator` and `device`.
    pub patch_sets: BTreeMap<String, Vec<String>>,
}

/// What every device file states, whatever else it holds: who the device is and what it can do. A device file has levels that follow
/// its tables: `[device]` + `[capabilities]` is an identity; `[patch]` adds patching; `[digikit]` adds the runtime target
/// (docs/design/device-data.md "Device levels").
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub short: String,
    pub name: String,
    pub capabilities: Capabilities,
    pub container: Container,
    pub acceptance: Option<Acceptance>,
    /// The file has `[patch]` (and `[device] syx_id`): patchkit can patch an image of it.
    pub patchable: bool,
    /// The file has `[digikit]`: the digikit runtime runs on it.
    pub runtime_target: bool,
}

/// `[capabilities]`: closed sets of words, checked against the tables by [`validate`].
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// `sharc` (a SHARC+ DSP runs the audio) or `none`.
    pub dsp: String,
    /// `hmac` (the container is sealed, key in `SEALS` of `syx.rs`) or `none`.
    pub seal: String,
    /// `stock` (run-time samples borrow stock sample slots), `ddr` (a reserved DDR range) or `none`.
    pub sample_pool: String,
    /// `path-fs` (a folder filesystem on the card), `slots` (card slots only), `none` or `unknown`.
    pub card: String,
}

/// `[container]`: section roles of the firmware container. Roles a device lacks are absent.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Container {
    pub main: u32,
    pub dsp: Option<u32>,
    pub cf2: Option<u32>,
    pub fpga: Option<u32>,
    /// Sections patchkit never changes.
    #[serde(default)]
    pub readonly: Vec<u32>,
}

/// `[acceptance]`: the device's test cell.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    /// The firmware version the acceptance tests run (a `[[firmware]]` version).
    pub firmware: String,
    pub ready_dir: String,
    pub sample_card: bool,
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
    capabilities: Option<Capabilities>,
    container: Option<Container>,
    acceptance: Option<Acceptance>,
    digikit: Option<Digikit>,
    patch: Option<Patch>,
    card: Option<CardDoc>,
    adapter: Option<AdapterDoc>,
    board: Option<BoardDoc>,
    #[serde(default)]
    firmware: Vec<FirmwareDoc>,
    limits: Option<LimitsDoc>,
    #[serde(default)]
    patch_sets: BTreeMap<String, Vec<String>>,
}
#[derive(Deserialize)]
struct CardDoc {
    path_fs: Option<toml::Value>,
}
#[derive(Deserialize)]
struct AdapterDoc {
    sample_pool: Option<SamplePoolDoc>,
}
#[derive(Deserialize)]
struct SamplePoolDoc {
    ddr_bytes: Option<u64>,
}
#[derive(Deserialize)]
struct BoardDoc {
    dsp_clock_hz: Option<u64>,
    dsp_blocks_per_second: Option<u64>,
}
/// The embedded copy has no `[[firmware]]` tables (`build.rs`); tests of the real files do.
#[derive(Deserialize)]
struct FirmwareDoc {
    dsp: Option<toml::Value>,
}
#[derive(Deserialize)]
struct LimitsDoc {
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
    dsp_area_bytes: Option<u64>,
}
#[derive(Deserialize)]
struct Patch {
    container_max: u64,
    dsp_stored_max: Option<u64>,
    dsp_depacked_max: Option<u64>,
    main_staged_max: u64,
    stream_id: u8,
    build_floor: Option<String>,
}

/// Words each capability may take.
const DSP_WORDS: [&str; 2] = ["sharc", "none"];
const SEAL_WORDS: [&str; 2] = ["hmac", "none"];
const SAMPLE_POOL_WORDS: [&str; 3] = ["stock", "ddr", "none"];
const CARD_WORDS: [&str; 4] = ["path-fs", "slots", "none", "unknown"];

fn word(n: &str, what: &str, v: &str, allowed: &[&str]) -> Result<()> {
    if allowed.contains(&v) {
        Ok(())
    } else {
        Err(err!(
            "devices/{n}: [capabilities] {what} = {v:?} is not one of {}",
            allowed.join(", ")
        ))
    }
}

/// The one validator: the capabilities agree with the tables of the file. A fact the table cannot show (`[[firmware]]` is not
/// embedded) is checked only where the table is there.
fn validate(n: &str, f: &File, caps: &Capabilities, c: &Container, short: &str) -> Result<()> {
    word(n, "dsp", &caps.dsp, &DSP_WORDS)?;
    word(n, "seal", &caps.seal, &SEAL_WORDS)?;
    word(n, "sample_pool", &caps.sample_pool, &SAMPLE_POOL_WORDS)?;
    word(n, "card", &caps.card, &CARD_WORDS)?;
    let bad = |m: String| Err(err!("devices/{n}: {m}"));
    let has_dsp = caps.dsp == "sharc";
    if !has_dsp {
        let dsp_limits = f
            .patch
            .as_ref()
            .is_some_and(|p| p.dsp_stored_max.is_some() || p.dsp_depacked_max.is_some())
            || f.board
                .as_ref()
                .is_some_and(|b| b.dsp_clock_hz.is_some() || b.dsp_blocks_per_second.is_some())
            || f.digikit
                .as_ref()
                .is_some_and(|k| k.dsp_area_bytes.is_some());
        if f.firmware.iter().any(|w| w.dsp.is_some()) || dsp_limits || c.dsp.is_some() {
            return bad("[capabilities] dsp = \"none\" forbids [[firmware]] dsp, dsp_* limits and [container] dsp".into());
        }
    } else if c.dsp.is_none() {
        return bad("[capabilities] dsp = \"sharc\" needs [container] dsp".into());
    } else if let Some(p) = &f.patch
        && (p.dsp_stored_max.is_none() || p.dsp_depacked_max.is_none())
    {
        return bad(
            "[capabilities] dsp = \"sharc\" needs [patch] dsp_stored_max and dsp_depacked_max"
                .into(),
        );
    }
    if caps.seal == "hmac" && !crate::syx::has_seal(short) {
        return bad(format!(
            "[capabilities] seal = \"hmac\" needs an entry for {short} in SEALS (packages/patchkit/src/syx.rs)"
        ));
    }
    let ddr = f
        .adapter
        .as_ref()
        .and_then(|a| a.sample_pool.as_ref())
        .map(|s| s.ddr_bytes.unwrap_or(0));
    match (caps.sample_pool.as_str(), ddr) {
        ("ddr", Some(b)) if b > 0 => {}
        ("ddr", _) => {
            return bad("sample_pool = \"ddr\" needs [adapter.sample_pool] ddr_bytes > 0".into());
        }
        ("stock", Some(0)) => {}
        ("stock", _) => {
            return bad(
                "sample_pool = \"stock\" needs [adapter.sample_pool] with ddr_bytes = 0".into(),
            );
        }
        (_, Some(_)) => return bad("sample_pool = \"none\" forbids [adapter.sample_pool]".into()),
        _ => {}
    }
    let path_fs = f.card.as_ref().is_some_and(|c| c.path_fs.is_some());
    match caps.card.as_str() {
        "path-fs" if !path_fs => return bad("card = \"path-fs\" needs [card.path_fs]".into()),
        "slots" if f.card.is_none() || path_fs => {
            return bad("card = \"slots\" needs [card] without [card.path_fs]".into());
        }
        "none" if f.card.is_some() => return bad("card = \"none\" forbids [card]".into()),
        _ => {}
    }
    let roles = [Some(c.main), c.dsp, c.cf2, c.fpga];
    let roles: Vec<u32> = roles.into_iter().flatten().collect();
    for (i, r) in roles.iter().enumerate() {
        if roles[..i].contains(r) {
            return bad(format!("[container] section {r} has two roles"));
        }
    }
    // a role section may be read-only (Syntakt), except the MAIN OS: patchkit has to change it
    if c.readonly.contains(&c.main) {
        return bad(format!("[container] main section {} is read-only", c.main));
    }
    if f.digikit.is_some() && f.patch.is_none() {
        return bad("[digikit] needs [patch]: a runtime target must be patchable".into());
    }
    Ok(())
}

/// Every device file's identity, and the patchable ones' [`DeviceInfo`]. A file without `[device]` (the LED maps) is skipped.
fn parse_all(files: &[(&str, &str)]) -> Result<(Vec<Identity>, Vec<DeviceInfo>)> {
    let mut ids: Vec<Identity> = vec![];
    let mut out: Vec<DeviceInfo> = vec![];
    for (n, text) in files {
        let f: File = toml::from_str(text).map_err(|e| err!("devices/{n}: {e}"))?;
        let Some(d) = &f.device else { continue };
        let (Some(caps), Some(cont)) = (&f.capabilities, &f.container) else {
            return Err(err!(
                "devices/{n}: [capabilities] and [container] are required in every device file"
            ));
        };
        validate(n, &f, caps, cont, &d.short)?;
        if ids.iter().any(|o| o.short == d.short) {
            return Err(err!("devices/{n}: device {} is described twice", d.short));
        }
        let identity = Identity {
            short: d.short.clone(),
            name: d.name.clone(),
            capabilities: caps.clone(),
            container: cont.clone(),
            acceptance: f.acceptance.clone(),
            patchable: f.patch.is_some(),
            runtime_target: f.digikit.is_some(),
        };
        ids.push(identity.clone());
        // identity only: nothing to patch
        let Some(p) = f.patch else { continue };
        let Some(syx_id) = d.syx_id else {
            return Err(err!("devices/{n}: [patch] needs [device] syx_id"));
        };
        if out.iter().any(|o| o.syx_id == syx_id) {
            return Err(err!("devices/{n}: syx_id {syx_id:#04x} is used twice"));
        }
        if let Some(k) = &f.digikit
            && out.iter().any(|o| o.digikit_id == Some(k.id))
        {
            return Err(err!("devices/{n}: [digikit] id {} is used twice", k.id));
        }
        out.push(DeviceInfo {
            short: d.short.clone(),
            name: d.name.clone(),
            syx_id,
            digikit_id: f.digikit.as_ref().map(|k| k.id),
            digikit_name: f.digikit.map(|k| k.name),
            identity,
            container_max: p.container_max,
            dsp_stored_max: p.dsp_stored_max.unwrap_or(0),
            dsp_depacked_max: p.dsp_depacked_max.unwrap_or(0),
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
            fx_slots: f.limits.and_then(|l| l.fx_slots),
            patch_sets: f.patch_sets,
        });
    }
    Ok((ids, out))
}

#[cfg(test)]
fn parse(files: &[(&str, &str)]) -> Result<Vec<DeviceInfo>> {
    parse_all(files).map(|r| r.1)
}

fn tables() -> &'static (Vec<Identity>, Vec<DeviceInfo>) {
    static ALL: OnceLock<(Vec<Identity>, Vec<DeviceInfo>)> = OnceLock::new();
    ALL.get_or_init(|| {
        parse_all(DEVICE_FILES).unwrap_or_else(|e| panic!("embedded device files: {e}"))
    })
}

/// Every device file's identity (patchable or not), in file-name order.
pub fn identities() -> &'static [Identity] {
    &tables().0
}

/// Every patchable device (a file with `[patch]`), in file-name order.
pub fn all() -> &'static [DeviceInfo] {
    &tables().1
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
            (dt2.syx_id, dt2.digikit_id, dt2.digikit_name.as_deref()),
            (0x14, Some(1), Some("dt2"))
        );
        assert_eq!(by_syx_id(0x15).unwrap().short, "dn2");
        assert!(by_short("nope").is_none());
    }

    const DT2: &str = include_str!("../../../devices/digitakt-ii.toml");
    const DN2: &str = include_str!("../../../devices/digitone-ii.toml");

    /// `text` with one `from` replaced by `to`; the edit must hit.
    fn edit(text: &str, from: &str, to: &str) -> String {
        assert!(text.contains(from), "{from}");
        text.replacen(from, to, 1)
    }

    #[test]
    fn the_device_files_state_their_capabilities() {
        let ids = identities();
        let dt2 = ids.iter().find(|i| i.short == "dt2").unwrap();
        assert_eq!(dt2.capabilities.card, "path-fs");
        assert_eq!(dt2.capabilities.sample_pool, "stock");
        assert!(dt2.runtime_target && dt2.patchable);
        let dn2 = ids.iter().find(|i| i.short == "dn2").unwrap();
        assert_eq!(dn2.capabilities.sample_pool, "ddr");
        assert_eq!(dn2.container.readonly, [2, 4, 8]);
        assert_eq!(dn2.acceptance.as_ref().unwrap().firmware, "1.12");
        // the real files, with their [[firmware]] tables, agree with their capabilities
        let both = [("dt2.toml", DT2), ("dn2.toml", DN2)];
        parse_all(&both).unwrap();
    }

    #[test]
    fn identity_only_and_patch_only_files_are_accepted() {
        let caps = "[capabilities]\ndsp = \"none\"\nseal = \"none\"\nsample_pool = \"none\"\ncard = \"unknown\"\n[container]\nmain = 1\n";
        let ident = format!("[device]\nname = \"X\"\nshort = \"x\"\n{caps}");
        let (ids, patchable) = parse_all(&[("x.toml", ident.as_str())]).unwrap();
        assert_eq!((ids.len(), patchable.len()), (1, 0));
        assert!(!ids[0].patchable && !ids[0].runtime_target);
        let patch = format!(
            "[device]\nname = \"X\"\nshort = \"x\"\nsyx_id = 0x30\n{caps}[patch]\ncontainer_max = 1\nmain_staged_max = 1\nstream_id = 1\n"
        );
        let (ids, patchable) = parse_all(&[("x.toml", patch.as_str())]).unwrap();
        assert!(ids[0].patchable && !ids[0].runtime_target);
        assert_eq!(patchable[0].digikit_id, None);
    }

    #[test]
    fn a_device_without_its_data_is_refused() {
        let bad = [("x.toml", "[device]\nname = \"X\"\nshort = \"x\"\n")];
        assert!(parse(&bad).is_err());
        let dup = [("a.toml", DT2), ("b.toml", DT2)];
        assert!(parse(&dup).is_err());
    }

    #[test]
    fn a_syntakt_like_identity_with_read_only_role_sections_parses() {
        let text = "[device]\nname = \"S\"\nshort = \"st\"\n[capabilities]\ndsp = \"none\"\nseal = \"none\"\nsample_pool = \"none\"\ncard = \"unknown\"\n[container]\nmain = 3\ncf2 = 7\nfpga = 1\nreadonly = [1, 2, 4, 6, 7, 8]\n";
        let (ids, patchable) = parse_all(&[("s.toml", text)]).unwrap();
        assert_eq!(
            (ids[0].container.cf2, ids[0].container.fpga),
            (Some(7), Some(1))
        );
        assert!(patchable.is_empty());
        // a runtime target must be patchable
        let rt = format!("{text}[digikit]\nid = 5\nname = \"st\"\n");
        let e = parse_all(&[("s.toml", rt.as_str())])
            .unwrap_err()
            .to_string();
        assert!(e.contains("must be patchable"), "{e}");
    }

    #[test]
    fn capabilities_that_disagree_with_the_tables_are_refused() {
        for (what, text) in [
            (
                "unknown word",
                edit(DT2, "card = \"path-fs\"", "card = \"floppy\""),
            ),
            (
                "path-fs without the table",
                edit(DN2, "card = \"slots\"", "card = \"path-fs\""),
            ),
            (
                "slots with a path fs",
                edit(DT2, "card = \"path-fs\"", "card = \"slots\""),
            ),
            (
                "ddr pool without ddr",
                edit(DT2, "sample_pool = \"stock\"", "sample_pool = \"ddr\""),
            ),
            (
                "stock pool with ddr",
                edit(DN2, "sample_pool = \"ddr\"", "sample_pool = \"stock\""),
            ),
            (
                "no dsp but dsp limits",
                edit(DT2, "dsp = \"sharc\"", "dsp = \"none\""),
            ),
            (
                "no dsp but firmware dsp",
                edit(DT2, "dsp = \"sharc\"", "dsp = \"none\""),
            ),
            (
                "main section is read-only",
                edit(DT2, "readonly = [2, 4, 8]", "readonly = [2, 3, 4, 8]"),
            ),
            (
                "one section, two roles",
                edit(DT2, "dsp = 7\n", "dsp = 3\n"),
            ),
        ] {
            assert!(parse_all(&[("x.toml", text.as_str())]).is_err(), "{what}");
        }
        // a seal needs a key in SEALS
        let mut unknown = edit(DT2, "short = \"dt2\"", "short = \"zz9\"");
        unknown = edit(&unknown, "id = 1\nname = \"dt2\"", "id = 9\nname = \"zz9\"");
        let e = parse_all(&[("x.toml", unknown.as_str())])
            .unwrap_err()
            .to_string();
        assert!(e.contains("SEALS"), "{e}");
        // capabilities are required
        let none = DT2.replace("[capabilities]", "[capabilities_x]");
        assert!(parse_all(&[("x.toml", none.as_str())]).is_err());
    }
}
