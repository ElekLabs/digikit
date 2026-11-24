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
    /// `[patch] main_dest`: where MAIN OS is unpacked to. `main_stage`: where the bootstrap stages the packed MAIN before it unpacks it in place.
    /// `main_margin_min`: the least in-place margin (bytes of unread stream ahead of the output) an image may have; default 8192.
    pub main_dest: u64,
    pub main_stage: u64,
    pub main_margin_min: u64,
    /// `[patch] stream_id`: the id byte of the OS UPGRADE header message (0x0f DT2, 0x10 DN2).
    pub stream_id: u8,
    /// `[patch] build_floor`: the four ASCII characters the OS validator compares the container's build word with (DT2 "0060");
    /// `None` when the OS has no floor (DN2).
    pub build_floor: Option<[u8; 4]>,
    /// `[limits] modules`: module slots of the runtime's tables.
    pub modules: Option<u64>,
    /// `[limits] fx_slots`: effects the device's chain takes (at most the contract's `DK_FX_MAX`).
    pub fx_slots: Option<u64>,
    /// Every key of `[limits]` (`param_rows`, `send_rows`, ...): what a reservation's `count` expression reads.
    pub limits: BTreeMap<String, u64>,
    /// `[patch_sets]`: named module sets as roots (the modules they require are added), e.g. `emulator` and `device`.
    pub patch_sets: BTreeMap<String, Vec<String>>,
    /// `[board] dsp_clock_hz` and `dsp_blocks_per_second`: the DSP block length the budget starts from.
    pub dsp_clock_hz: Option<u64>,
    pub dsp_blocks_per_second: Option<u64>,
    /// `[adapter.machine] voices`: voices of the machine adapter.
    pub machine_voices: Option<u64>,
    /// `[protect] windows`: address windows `[lo, hi)` that only the firmware and `core/*` drive (flash, FPGA and peer buses, flash controller
    /// registers). The packer refuses a content module that holds an address in one of them (`crate::protect`).
    pub protect: Vec<[u64; 2]>,
    /// Every boolean of a nested table of the device file, by dotted path (`card.guard.enabled`): the switches that a module's `needed_when` reads.
    pub switches: BTreeMap<String, bool>,
}

/// Every boolean of a nested table, by dotted path (`card.guard.enabled`). A boolean at the top level of the file is not a switch.
pub fn bool_switches(t: &toml::Table) -> BTreeMap<String, bool> {
    fn walk(t: &toml::Table, prefix: &str, out: &mut BTreeMap<String, bool>) {
        for (k, v) in t {
            match v {
                toml::Value::Table(sub) => {
                    let path = if prefix.is_empty() {
                        k.clone()
                    } else {
                        format!("{prefix}.{k}")
                    };
                    walk(sub, &path, out);
                }
                toml::Value::Boolean(b) if !prefix.is_empty() => {
                    out.insert(format!("{prefix}.{k}"), *b);
                }
                _ => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(t, "", &mut out);
    out
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
    /// Roles (`dsp`, `cf2`, `fpga`) of read-only sections that a module may store packed when it declares that it unpacks them (`[[repack]]` of a
    /// `module.toml`). The decoded bytes stay the stock bytes; only the stored form shrinks.
    #[serde(default)]
    pub repack: Vec<String>,
}

impl Container {
    /// The section of a role (`main`, `dsp`, `cf2`, `fpga`), if the device has one.
    pub fn role_section(&self, role: &str) -> Option<u32> {
        match role {
            "main" => Some(self.main),
            "dsp" => self.dsp,
            "cf2" => self.cf2,
            "fpga" => self.fpga,
            _ => None,
        }
    }
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

    /// Any key of the device file's `[limits]`; `None` when the file has no such key.
    pub fn limit_of(&self, name: &str) -> Option<u64> {
        self.limits.get(name).copied()
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

/// The part of `[rows]` a reservation count reads: each `[[rows.block]]` is `per` times the `[limits]` key `limit`, named `rows_b<N>` (N from 1,
/// in file order) in a count expression. The other keys of `[rows]` and of a block belong to `patchkit gen` (`devgen.rs`).
#[derive(Deserialize)]
struct RowsBlocksDoc {
    #[serde(default)]
    block: Vec<RowsBlockSize>,
}
#[derive(Deserialize)]
struct RowsBlockSize {
    limit: String,
    #[serde(default = "one")]
    per: u64,
}
fn one() -> u64 {
    1
}

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
    rows: Option<RowsBlocksDoc>,
    #[serde(default)]
    patch_sets: BTreeMap<String, Vec<String>>,
    protect: Option<ProtectDoc>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtectDoc {
    windows: Vec<[u64; 2]>,
}
#[derive(Deserialize)]
struct CardDoc {
    path_fs: Option<toml::Value>,
}
#[derive(Deserialize)]
struct AdapterDoc {
    machine: Option<MachineDoc>,
    sample_pool: Option<SamplePoolDoc>,
}
#[derive(Deserialize)]
struct MachineDoc {
    voices: Option<u64>,
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
#[derive(Deserialize)]
struct FirmwareDoc {
    dsp: Option<toml::Value>,
}
/// `[limits]`: every value is a non-negative integer; a key with another type is refused by name.
struct LimitsDoc(BTreeMap<String, u64>);
impl<'de> Deserialize<'de> for LimitsDoc {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        let table = BTreeMap::<String, toml::Value>::deserialize(d)?;
        let mut out = BTreeMap::new();
        for (key, v) in table {
            match v.as_integer().and_then(|n| u64::try_from(n).ok()) {
                Some(n) => {
                    out.insert(key, n);
                }
                None => {
                    return Err(serde::de::Error::custom(format!(
                        "[limits] {key} must be a non-negative integer, found {v}"
                    )));
                }
            }
        }
        Ok(LimitsDoc(out))
    }
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
    main_dest: u64,
    main_stage: u64,
    main_margin_min: Option<u64>,
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

/// The one validator: the capabilities agree with the tables of the file. `[[firmware]]` tables are embedded with the rest of the file, so the
/// same checks run in the browser build.
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
    // a role section may be read-only (a device with a bootstrap or peer CPU), except the MAIN OS: patchkit has to change it
    if c.readonly.contains(&c.main) {
        return bad(format!("[container] main section {} is read-only", c.main));
    }
    // the peer CPU and FPGA sections are never patched: they must be declared read-only
    for (role, id) in [("cf2", c.cf2), ("fpga", c.fpga)] {
        if let Some(id) = id.filter(|id| !c.readonly.contains(id)) {
            return bad(format!(
                "[container] {role} section {id} must be in readonly"
            ));
        }
    }
    // a section that a module may store packed is a role section that is read-only in content
    for role in &c.repack {
        if role == "main" {
            return bad("[container] repack: the main section is changed, not repacked".into());
        }
        match c.role_section(role) {
            None => {
                return bad(format!(
                    "[container] repack: the device has no {role} section"
                ));
            }
            Some(id) if !c.readonly.contains(&id) => {
                return bad(format!(
                    "[container] repack {role}: section {id} must be in readonly"
                ));
            }
            Some(_) => {}
        }
    }
    if f.digikit.is_some() && f.patch.is_none() {
        return bad("[digikit] needs [patch]: a runtime target must be patchable".into());
    }
    if let Some(p) = &f.protect {
        for [lo, hi] in &p.windows {
            if lo >= hi || *hi > 1 << 32 {
                return bad(format!(
                    "[protect] window [{lo:#x}, {hi:#x}) needs lo < hi <= 0x100000000"
                ));
            }
        }
    }
    Ok(())
}

/// Every device file's identity, and the patchable ones' [`DeviceInfo`]. A file without `[device]` (the LED maps) is skipped.
fn parse_all(files: &[(&str, &str)]) -> Result<(Vec<Identity>, Vec<DeviceInfo>)> {
    let mut ids: Vec<Identity> = vec![];
    let mut out: Vec<DeviceInfo> = vec![];
    for (n, text) in files {
        let f: File = toml::from_str(text).map_err(|e| err!("devices/{n}: {e}"))?;
        let raw: toml::Table = toml::from_str(text).map_err(|e| err!("devices/{n}: {e}"))?;
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
            main_dest: p.main_dest,
            main_stage: p.main_stage,
            main_margin_min: p.main_margin_min.unwrap_or(8192),
            stream_id: p.stream_id,
            build_floor: match p.build_floor {
                None => None,
                Some(s) => {
                    Some(<[u8; 4]>::try_from(s.as_bytes()).map_err(|_| {
                        err!("devices/{n}: [patch] build_floor must be 4 characters")
                    })?)
                }
            },
            modules: f.limits.as_ref().and_then(|l| l.0.get("modules").copied()),
            fx_slots: f.limits.as_ref().and_then(|l| l.0.get("fx_slots").copied()),
            limits: {
                let mut limits = f.limits.map(|l| l.0).unwrap_or_default();
                for (i, b) in f.rows.iter().flat_map(|r| &r.block).enumerate() {
                    let base = limits.get(&b.limit).copied().ok_or_else(|| {
                        err!(
                            "devices/{n}: [[rows.block]] names {:?}, which is no key of [limits]",
                            b.limit
                        )
                    })?;
                    limits.insert(format!("rows_b{}", i + 1), b.per * base);
                }
                limits
            },
            patch_sets: f.patch_sets,
            dsp_clock_hz: f.board.as_ref().and_then(|b| b.dsp_clock_hz),
            dsp_blocks_per_second: f.board.as_ref().and_then(|b| b.dsp_blocks_per_second),
            machine_voices: f.adapter.and_then(|a| a.machine).and_then(|m| m.voices),
            protect: f.protect.map(|p| p.windows).unwrap_or_default(),
            switches: bool_switches(&raw),
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

/// Whether `key` is an integer in `[limits]` or `[digikit]` of every device file that is a runtime target (has `[digikit]`): the names a service
/// manifest may write as `profile:<key>`.
pub fn has_profile_key(key: &str) -> bool {
    let mut any = false;
    for (n, text) in DEVICE_FILES {
        let Ok(raw) = toml::from_str::<toml::Table>(text) else {
            panic!("devices/{n}: embedded device file does not parse")
        };
        if !raw.contains_key("digikit") {
            continue;
        }
        let found = ["limits", "digikit"].iter().any(|t| {
            raw.get(*t)
                .and_then(|v| v.get(key))
                .is_some_and(|v| v.is_integer())
        });
        if !found {
            return false;
        }
        any = true;
    }
    any
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
    fn profile_keys_are_integers_of_every_runtime_device_file() {
        assert!(has_profile_key("fx_slots") && has_profile_key("modules"));
        assert!(!has_profile_key("name"), "a string is no profile key");
        assert!(!has_profile_key("no_such_key"));
    }

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
        // every real file, with its [[firmware]] tables, agrees with its capabilities
        parse_all(DEVICE_FILES).unwrap();
    }

    #[test]
    fn identity_only_and_patch_only_files_are_accepted() {
        let caps = "[capabilities]\ndsp = \"none\"\nseal = \"none\"\nsample_pool = \"none\"\ncard = \"unknown\"\n[container]\nmain = 1\n";
        let ident = format!("[device]\nname = \"X\"\nshort = \"x\"\n{caps}");
        let (ids, patchable) = parse_all(&[("x.toml", ident.as_str())]).unwrap();
        assert_eq!((ids.len(), patchable.len()), (1, 0));
        assert!(!ids[0].patchable && !ids[0].runtime_target);
        let patch = format!(
            "[device]\nname = \"X\"\nshort = \"x\"\nsyx_id = 0x30\n{caps}[patch]\ncontainer_max = 1\nmain_staged_max = 1\nstream_id = 1\nmain_dest = 16\nmain_stage = 32\n"
        );
        let (ids, patchable) = parse_all(&[("x.toml", patch.as_str())]).unwrap();
        assert!(ids[0].patchable && !ids[0].runtime_target);
        assert_eq!(patchable[0].digikit_id, None);
    }

    #[test]
    fn a_limits_key_that_is_no_integer_is_refused_by_name() {
        let dev_file = DEVICE_FILES
            .iter()
            .find(|(_, t)| t.contains("param_rows = 16"))
            .unwrap()
            .1;
        let text = edit(dev_file, "param_rows = 16", "param_rows = \"sixteen\"");
        let e = parse(&[("limits.toml", text.as_str())])
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("[limits] param_rows must be a non-negative integer"),
            "{e}"
        );
        let text = edit(dev_file, "param_rows = 16", "param_rows = -1");
        assert!(parse(&[("limits.toml", text.as_str())]).is_err());
    }

    #[test]
    fn the_rows_blocks_give_the_sizes_a_reservation_count_reads() {
        let dev_file = DEVICE_FILES
            .iter()
            .find(|(_, t)| t.contains("[[rows.block]]"))
            .unwrap()
            .1;
        let dev = parse(&[("rows.toml", dev_file)]).unwrap().remove(0);
        assert_eq!(dev.limit_of("rows_b1"), dev.limit_of("param_rows"));
        assert_eq!(
            dev.limit_of("rows_b2"),
            dev.limit_of("fx_slots").map(|n| 8 * n)
        );
        assert_eq!(dev.limit_of("rows_b3"), dev.limit_of("send_rows"));
        // a device file without `[rows]` parses and has no block sizes
        let text = dev_file
            .replace("[rows]", "[rows_gone]")
            .replace("[[rows.block]]", "[[rows_gone.block]]");
        let dev = parse(&[("norows.toml", text.as_str())]).unwrap().remove(0);
        assert!(dev.limit_of("rows_b1").is_none());
        // a block that names no `[limits]` key is refused at load
        let text = edit(
            dev_file,
            "limit = \"param_rows\"",
            "limit = \"no_such_limit\"",
        );
        let e = parse(&[("rows.toml", text.as_str())])
            .unwrap_err()
            .to_string();
        assert!(e.contains("[[rows.block]] names \"no_such_limit\""), "{e}");
    }

    #[test]
    fn a_device_without_its_data_is_refused() {
        let bad = [("x.toml", "[device]\nname = \"X\"\nshort = \"x\"\n")];
        assert!(parse(&bad).is_err());
        let dup = [("a.toml", DT2), ("b.toml", DT2)];
        assert!(parse(&dup).is_err());
    }

    #[test]
    fn a_peer_cpu_identity_with_read_only_role_sections_parses() {
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
    fn repack_names_a_read_only_role_section() {
        let text = "[device]\nname = \"S\"\nshort = \"st\"\n[capabilities]\ndsp = \"none\"\nseal = \"none\"\nsample_pool = \"none\"\ncard = \"unknown\"\n[container]\nmain = 3\ncf2 = 7\nfpga = 1\nreadonly = [1, 2, 4, 6, 7, 8]\n";
        let ok = format!("{text}repack = [\"cf2\"]\n");
        let (ids, _) = parse_all(&[("s.toml", ok.as_str())]).unwrap();
        assert_eq!(ids[0].container.repack, ["cf2"]);
        // the role's section must be read-only: the same file without 7 in readonly is already refused, so test the other roles
        let no_role = text.replace("cf2 = 7\n", "") + "repack = [\"cf2\"]\n";
        assert!(parse_all(&[("s.toml", no_role.as_str())]).is_err());
        let unknown = format!("{text}repack = [\"cpu9\"]\n");
        assert!(parse_all(&[("s.toml", unknown.as_str())]).is_err());
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
                "cf2 section not read-only",
                edit(DT2, "dsp = 7\n", "dsp = 7\ncf2 = 9\n"),
            ),
            (
                "fpga section not read-only",
                edit(DT2, "dsp = 7\n", "dsp = 7\nfpga = 9\n"),
            ),
            (
                "repack of a role the device lacks",
                edit(
                    DT2,
                    "readonly = [2, 4, 8]",
                    "readonly = [2, 4, 8]\nrepack = [\"cf2\"]",
                ),
            ),
            (
                "repack of the main section",
                edit(
                    DT2,
                    "readonly = [2, 4, 8]",
                    "readonly = [2, 4, 8]\nrepack = [\"main\"]",
                ),
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
