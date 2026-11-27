//! Assembly tests (docs/plans/test-architecture.md T2 step 4): the seams between separately built artifacts, run on the host with the real packed outputs.
//!
//! Nothing here is a fake of an artifact. The files are the ones the build leaves in `dist/`: the packed fixtures (`dist/modules/<device>/*.dkm`), the service files
//! (`core/services/{fx,machines,lfo}`, `test/svc-test`) as the card holds them (the packed file of `dist/modules/<device>/`, and the port file of `dist/ports/` beside them), the committed
//! runtime object (`runtime/core/build/runtime.dko.json`, the immediate of its ABI check) and the info table of the patched image
//! (`dist/bytes/images/<device>/set-<set>.info`, written by `patchkit:device-images` from the plan of `patchkit apply`). They go through the code of the boot:
//! the search for providers on the card, the package manager's boot (the graph's order and needs, the rules of the core for owners of kinds and keys, capacity limits from the
//! `cap_<kind>` entries), the role checks of a provider file (ABI word, stock bytes of every hook site), the install of its hooks and slots over `FakePlatform`, and the load of
//! a module without a DSP section.
//!
//! What the host cannot show stays out: no ColdFire code runs (in the loader cases the `init` of a service replays what its header table declares; one case runs the glue of each Rust service file natively and compares), and the DSP part of a module is
//! not loaded (its DSP loader needs the DSP). One case per seam shows that it fits and one the refusal that is typical for it. The edge cases of each part are unit
//! tests of that part; an assertion that needs a hand-built table or a changed rule belongs there, not here.
//!
//! The tests need the outputs of the Nx target `core/runtime:test-assembly` (its `dependsOn`). A device without its firmware image is skipped, as its device tests are.
use super::*;
use crate::fake_platform::{Call, FakePlatform};
use crate::package::lifecycle::{Manager, RtManifest, Session};
use crate::test_lock::LOCK;
use dkrt_lifecycle::{LifecycleError, PackageId, Report, State};
use dkneeds::keys::{name_hash, tag, Keys};
use dkrt_abi::plan::{Reason, Verdict};
use dkrt_abi::profiles;
use dkrt_abi::service::Declare;
use dkrt_sdk::proto::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

mod lfo_closure_reload;
mod lifecycle_events;
mod settings_page;
mod workbench;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../..")
}

#[derive(Clone, Copy)]
pub(crate) struct Dev {
    pub(crate) name: &'static str,
    profile: dkrt_abi::platform::DeviceProfile,
    /// The stock firmware the images are made from (the acceptance cells); its sites' stock bytes come from `patchkit port stock`.
    firmware: &'static str,
    /// The device file whose `[patch_sets]` name the roots of each image set.
    toml: &'static str,
}

const DT2: Dev = Dev { name: "dt2", profile: profiles::DT2, firmware: "firmware/Digitakt_II_OS1.17.syx", toml: "devices/digitakt-ii.toml" };
const DN2: Dev = Dev { name: "dn2", profile: profiles::DN2, firmware: "firmware/Digitone_II_OS1.12.syx", toml: "devices/digitone-ii.toml" };
pub(crate) const DEVICES: [Dev; 2] = [DT2, DN2];

impl Dev {
    /// The name of the port file of the firmware: `<device>-<version>`, the version from the firmware file's name (`..._OS1.17.syx`).
    fn port(&self) -> String {
        let stem = self.firmware.rsplit('/').next().unwrap().trim_end_matches(".syx");
        std::format!("{}-{}", self.name, stem.rsplit("OS").next().unwrap())
    }

    /// False (with a note) when the firmware is not here: its images are not made, and nothing of this device can be assembled.
    pub(crate) fn has_firmware(&self) -> bool {
        let ok = repo().join(self.firmware).exists();
        if !ok {
            eprintln!("SKIP {}: {} is not here", self.name, self.firmware);
        }
        ok
    }
}

fn read(path: &Path, make: &str) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}; run `{make}`", path.display()))
}

// ---- the artifacts ------------------------------------------------------------------------------------------------------------

/// The packed file of module `id`'s leaf name: a `.dkm`, or a `.dks` for a service.
pub(crate) fn packed(dev: Dev, leaf: &str, ext: &str, nx: &str) -> Vec<u8> {
    read(&repo().join(format!("dist/modules/{}/{leaf}.{ext}", dev.name)), &format!("pnpm nx:pilot run {nx}:pack"))
}

/// A service file as the card holds it: the packed file, which names the firmware only by imports and holds no firmware bytes.
fn service_file(dev: Dev, leaf: &str, nx: &str) -> Vec<u8> {
    packed(dev, leaf, "dks", nx)
}

/// The bytes the fixture `local-patch` declares for its patch (`BOUND_PATCH` of tests/fixtures/modules/local-patch/src/lib.rs): `init` is replayed here, so the value is the test's.
pub(super) const PATCH_TEST_VALUE: u32 = 0xfffe;

/// The stock bytes of every hook site a service takes, from the local firmware (`patchkit port stock`, which reads the image and the committed port data; the bytes go
/// into a fake firmware here and are written nowhere): hook name hash to (site address, displaced bytes). The port file holds only their check values.
pub(super) fn stock_sites(dev: Dev) -> std::collections::BTreeMap<u32, (u32, Vec<u8>)> {
    let out = std::process::Command::new(repo().join("dist/patchkit/patchkit"))
        .current_dir(repo())
        .args(["--root", ".", "port", "stock", "--firmware"])
        .arg(repo().join(dev.firmware))
        .arg("--port")
        .arg(repo().join(std::format!("ports/{}.toml", dev.port())))
        .output()
        .unwrap_or_else(|e| panic!("patchkit port stock: {e}; run `pnpm nx:pilot run patchkit:cli`"));
    assert!(out.status.success(), "patchkit port stock: {}", String::from_utf8_lossy(&out.stderr));
    let table: toml::Table = String::from_utf8(out.stdout).unwrap().parse().unwrap();
    table
        .iter()
        .map(|(hook, v)| {
            let hex = v["stock"].as_str().unwrap();
            let bytes: Vec<u8> = (0..hex.len() / 2).map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap()).collect();
            (name_hash(hook), (v["addr"].as_integer().unwrap() as u32, bytes))
        })
        .collect()
}

/// The port file of the test image for the device's firmware, as the card holds it: the production port file and the hooks of the fixture service `test/svc-test`.
fn port_file(dev: Dev) -> Vec<u8> {
    read(&repo().join(format!("dist/ports-test/{}.dkport", dev.port())), "pnpm nx:pilot run patchkit:ports-test")
}

fn be(f: &[u8], at: u32) -> u32 {
    u32::from_be_bytes(f[at as usize..at as usize + 4].try_into().unwrap())
}

/// The header table of a file.
pub(crate) fn table_of(f: &[u8]) -> Vec<u8> {
    let (off, len) = (be(f, dkneeds::H_REG_OFF) as usize, be(f, dkneeds::H_REG_SIZE) as usize);
    f[off..off + len].to_vec()
}

/// The `"hex"` strings of an object file (`*.dko.json`), in file order.
fn hex_sections(json: &str) -> Vec<&str> {
    json.split("\"hex\": \"").skip(1).map(|s| s.split('"').next().unwrap()).collect()
}

/// The ABI word the object of a service carries (`dk_svc_abi`, an 8-digit hex string in the `.rodata.dk_svc_abi` section).
fn object_abi(json: &str) -> &str {
    let at = json.find("\".rodata.dk_svc_abi\"").expect("no dk_svc_abi section");
    let from = json[..at].rfind("\"hex\": \"").expect("no hex before dk_svc_abi") + 8;
    json[from..].split('"').next().unwrap()
}

/// The committed object of the system package (its sections, joined): `cmpi.l #SVC_ABI,d0` of `abi_ok` is `0c80` and the word. The loader, and so the check, is package
/// code since RC2s-2 step d (the resident object holds none). A host build has another `SVC_ABI` (64-bit sizes), so the code of the device is the object.
fn package_object() -> String {
    let json = read(&repo().join("runtime/core/build/package.dko.json"), "patchkit build runtime/core");
    hex_sections(std::str::from_utf8(&json).unwrap()).join("|")
}

/// The registry of the device: the ABI check of the package object, the rows of the runtime's own registry.
pub(super) struct DeviceRegistry {
    runtime: String,
}

impl Registry for DeviceRegistry {
    fn abi_ok(&self, fingerprint: u32) -> bool {
        fingerprint == 0 || self.runtime.contains(&std::format!("0c80{fingerprint:08x}"))
    }
    unsafe fn add(&mut self, rec: usize, row: Option<crate::package::provider_rows::RowSource<'_>>, order: Option<u32>) -> i32 {
        RealRegistry.add(rec, row, order)
    }
    unsafe fn remove(&mut self, rec: usize) {
        // a module's registry rows are the scripted cleanup tests' (`registry_remove` asserts their event order); a provider's row is real
        crate::package::provider_rows::remove(rec)
    }
    unsafe fn module_begin(&mut self, rec: usize) {
        RealRegistry.module_begin(rec)
    }
    unsafe fn module_commit(&mut self, rec: usize, c: &dkrt_abi::service::ModuleCommitCtx) -> i32 {
        RealRegistry.module_commit(rec, c)
    }
}

/// The modules of patch set `set` in the device file: its roots (`[patch_sets]`) and what they require, transitively (`requires` of each `module.toml`), as
/// `Library::with_requires` of patchkit expands them. patchkit is not a dependency here (it pulls the emulator crates and a second lock), so the walk is this one.
fn set_modules(dev: Dev, set: &str) -> Vec<String> {
    let text = String::from_utf8(read(&repo().join(dev.toml), "git checkout devices")).unwrap();
    let file: toml::Table = text.parse().unwrap_or_else(|e| panic!("{}: {e}", dev.toml));
    let roots = file["patch_sets"].get(set).unwrap_or_else(|| panic!("{}: no patch set {set}", dev.name));
    let requires = module_requires();
    let mut all: Vec<String> = Vec::new();
    let mut todo: Vec<String> = strings(roots);
    while let Some(id) = todo.pop() {
        if !all.contains(&id) {
            todo.extend(requires.get(&id).unwrap_or_else(|| panic!("{}: set {set} names {id}, which no shipped module.toml declares", dev.name)).clone());
            all.push(id);
        }
    }
    all
}

fn strings(v: &toml::Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

/// The `requires` of every shipped module, by id (`[module] id` and `requires` of each `module.toml`).
fn module_requires() -> std::collections::BTreeMap<String, Vec<String>> {
    fn walk(dir: &Path, out: &mut std::collections::BTreeMap<String, Vec<String>>) {
        for e in std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()) {
            if e.is_dir() {
                if !["build", "target", "node_modules"].contains(&e.file_name().unwrap().to_str().unwrap()) {
                    walk(&e, out);
                }
            } else if e.file_name().unwrap() == "module.toml" {
                let t: toml::Table = std::fs::read_to_string(&e).unwrap().parse().unwrap_or_else(|x| panic!("{}: {x}", e.display()));
                let m = &t["module"];
                out.insert(m["id"].as_str().unwrap().to_string(), m.get("requires").map(strings).unwrap_or_default());
            }
        }
    }
    let mut out = std::collections::BTreeMap::new();
    let repo = repo();
    for root in ["modules", "runtime", "services"] {
        walk(&repo.join(root), &mut out);
    }
    out
}

/// The module an SDK import needs (`requires` of its `[[import]]` in `sdk/sdk.toml`, the source of the runtime's `optional` link list); `None` for an import
/// without `requires`.
fn adapter_of(import: &str) -> Option<String> {
    // the SDK's import table, read once: `import_addr` asks for every import of every file a test loads
    static REQUIRES: std::sync::OnceLock<std::collections::HashMap<String, String>> = std::sync::OnceLock::new();
    REQUIRES
        .get_or_init(|| {
            let text = String::from_utf8(read(&repo().join("sdk/sdk.toml"), "git checkout sdk")).unwrap();
            let sdk: toml::Table = text.parse().unwrap();
            let imports = sdk["import"].as_array().unwrap();
            imports
                .iter()
                .filter_map(|i| Some((i["name"].as_str()?.to_string(), i.get("requires")?.as_str()?.to_string())))
                .collect()
        })
        .get(import)
        .cloned()
}

/// The info table of a patched image (`set-<set>.info`): name, hash, kind (0 data, 1 slot, 2 DSP slot), address (the index in the slot block for a DSP slot) and the words of a data entry.
pub(super) struct Image {
    entries: Vec<(String, u32, u32, u32, Vec<u32>)>,
    /// The modules of the set (`[patch_sets]` of the device file and what they require): the adapters the image has.
    modules: Vec<String>,
}

impl Image {
    pub(super) fn read(dev: Dev, set: &str) -> Image {
        let path = repo().join(format!("dist/bytes/images/{}/set-{set}.info", dev.name));
        let text = String::from_utf8(read(&path, "pnpm nx:pilot run patchkit:device-images")).unwrap();
        let hex = |s: &str| u32::from_str_radix(s, 16).unwrap();
        let entries = text
            .lines()
            .map(|l| {
                let w: Vec<&str> = l.split(' ').collect();
                (w[0].to_string(), hex(w[1]), w[2].parse().unwrap(), hex(w[3]), w[4..].iter().map(|x| hex(x)).collect())
            })
            .collect();
        Image { entries, modules: set_modules(dev, set) }
    }

    /// The platform of the device with this image's table. `limits`: replacement values of the `cap_<kind>` words.
    fn platform(&self, dev: Dev, limits: &[(u8, u32)]) -> FakePlatform {
        let p = FakePlatform::new(dev.profile);
        for (_, hash, kind, at, words) in &self.entries {
            let mut words = if words.is_empty() { std::vec![0] } else { words.clone() };
            if let Some((_, v)) = limits.iter().find(|(k, _)| *hash == cap_hash(*k)) {
                words[0] = *v;
            }
            p.info_entries.borrow_mut().push((*hash, words));
            if *kind == 0 {
                p.info_data.borrow_mut().push(*hash);
            }
            if *kind == 2 {
                p.info_dsp.borrow_mut().push((*hash, *at));
            }
        }
        p
    }

    /// The words of the data entry `name` of the table.
    fn words(&self, name: &str) -> Vec<u32> {
        self.entries.iter().find(|e| e.0 == name).unwrap_or_else(|| panic!("the image table has no {name}")).4.clone()
    }

    fn has(&self, hash: u32) -> bool {
        self.entries.iter().any(|e| e.1 == hash && !e.4.is_empty())
    }
}

fn cap_hash(kind: u8) -> u32 {
    CAP_INFO_HASHES[CAP_KINDS.iter().position(|k| *k == kind).unwrap()]
}

/// The capacity before anything loads, as `dkrt_capacity` builds it: the limits of the kinds the runtime counts itself are as large as they can be here (their
/// unit tests own them), the rest come from the image's `cap_<kind>` entries and the rows of the services loaded so far.
pub(super) unsafe fn capacity(p: &FakePlatform) -> Vec<DkrtCap> {
    let table = crate::package::handlers::services();
    (0..CAP_KINDS.len())
        .map(|k| {
            let kind = CAP_KINDS[k];
            let big = 1 << 20;
            let base = if kind == CAPK_MODULES {
                (kind, MAX_MODULES as u32, 0, 0)
            } else if kind == CAPK_SELECTION {
                (kind, SEL_MAX as u32, 0, SEL_MAX as u32)
            } else if kind == CAPK_DSP_COST {
                (kind, p.profile.dsp_headroom, 0, 0)
            } else if kind == CAPK_DSP_VOICES {
                (kind, p.profile.dsp_voices, 0, 0)
            } else if [CAPK_ARENA, CAPK_STAGE, CAPK_DSP_AREA, CAPK_STORE, CAPK_SETTINGS].contains(&kind) {
                (kind, big, 0, big)
            } else {
                (kind, 0, 0, 0)
            };
            let v = crate::package::capacity::kind_value(p, &table, k, base);
            DkrtCap { kind: kind as u32, limit: v.1, used: v.2, largest: v.3 }
        })
        .collect()
}

// ---- the loader over a card ---------------------------------------------------------------------------------------------------

/// The row of the provider fixture with a tick part one version newer than the core knows.
fn newer_ping_row() -> &'static dkrt_abi::provider::ProviderRow {
    use dkrt_abi::provider::{LifecycleV1, PartRef, ProviderRow, TickV1, LIFECYCLE_NEWEST, PART_LIFECYCLE, PART_TICK, TICK_NEWEST};
    use crate::package::core_api::RealCore;
    static LIFE: LifecycleV1 = LifecycleV1::of::<dkrt_fixture_local_ping::LocalPing, RealCore>();
    static TICK: TickV1 = TickV1::of::<dkrt_fixture_local_ping::LocalPing, RealCore>();
    static PARTS: [PartRef; 2] = [
        PartRef::new(PART_LIFECYCLE, LIFECYCLE_NEWEST, &LIFE as *const _ as *const ()),
        PartRef::new(PART_TICK, TICK_NEWEST + 1, &TICK as *const _ as *const ()),
    ];
    static ROW: ProviderRow = ProviderRow::new(dkrt_abi::provider::name_hash("local-ping"), &PARTS);
    &ROW
}

fn other_name_ping_row() -> &'static dkrt_abi::provider::ProviderRow {
    use dkrt_abi::provider::{LifecycleV1, PartRef, ProviderRow, LIFECYCLE_NEWEST, PART_LIFECYCLE};
    use crate::package::core_api::RealCore;
    static LIFE: LifecycleV1 = LifecycleV1::of::<dkrt_fixture_local_ping::LocalPing, RealCore>();
    static PARTS: [PartRef; 1] = [PartRef::new(PART_LIFECYCLE, LIFECYCLE_NEWEST, &LIFE as *const _ as *const ())];
    static ROW: ProviderRow = ProviderRow::new(dkrt_abi::provider::name_hash("local-pong"), &PARTS);
    &ROW
}

/// The store and the arena of a card: every import resolves against the runtime's own tables (`RealImages`); `init` of a service file replays what its header
/// table declares (no ColdFire code runs on the host).
pub(super) struct Card {
    pub(super) files: Vec<(Vec<u8>, Vec<u8>)>,
    staged: Vec<u8>,
    staged_path: Vec<u8>,
    pub(super) live: Vec<(u32, Vec<u128>)>,
    allocs: u32,
    /// The blocks given back: (address, bytes). Only a load that fails flushes a block after its free (its cleanup), so a golden shows `freed:` only for such a path.
    freed: Vec<(u32, u32)>,
    /// The runtime id of the device whose `init` the card replays.
    device: u32,
    /// The builder calls the `init` of a module makes (`replay_init`): the host runs no ColdFire code, so a case that needs a module's declarations lists them here.
    pub(super) init_calls: Vec<Builder>,
    /// The effects and machines service files declare the row of the real service (its builder calls and commit checks), not the empty one every service file gets here.
    pub(super) real_rows: bool,
    /// `init` declares the handler of every kind 4 bytes off the one the header table carries (a service whose `init` and file disagree).
    pub(super) skew_kind_handler: bool,
    /// The replayed `init` of a service declares its patches with another length than the port file's site (a file whose `service!` list and port disagree).
    pub(super) skew_patch_len: bool,
    /// The `init` and `fini` the loader entered, in order: `(is_fini, record)`.
    pub(super) hooks: Vec<(bool, usize)>,
    /// The records whose service file provides a `ProviderRow` (the fixture `local-ping`, linked into the test binary): their `fini` stops the instance.
    provider_recs: Vec<usize>,
    /// The provider fixture declares a row whose tick part is a version newer than this core knows (a thin variant of the fixture: the host runs no file code, so the row is the
    /// test's).
    pub(super) newer_parts: bool,
    /// The `local-ping` row declares another service name than the file PROVIDES.
    pub(super) wrong_name: bool,
    /// The device addresses of the runtime's functions that the imports of a service file name (`svc_import_addr`).
    imports: crate::fake_platform::address_map::AddressMap,
}

/// A call replayed during module `init`. Machines and effects use a key index, and Workbench uses descriptor bytes from the staged file.
#[derive(Clone, Copy)]
pub(super) enum Builder {
    Machine(u32),
    Fx(u32),
    Workbench { offset: usize, len: usize },
}

impl Card {
    /// `addr..addr+len` as a place in the arena of the card: `blk<slot>+<offset>` inside a live block, `freed:blk<slot>+<offset>` inside a block that went back (a stand-in
    /// address, never a host address). `None`: a use after free, an access past a block's end or a host pointer. A load that fails is flushed before its block goes back.
    pub(super) fn arena_ref(&self, addr: u32, len: u32) -> Option<String> {
        let at = |(base, size): (u32, u32)| (addr >= base && addr - base < size && len <= size - (addr - base)).then(|| std::format!("blk{}+{}", base / 0x0010_0000, addr - base));
        if let Some(r) = self.live.iter().map(|b| (b.0, b.1.len() as u32 * 16)).find_map(at) {
            return Some(r);
        }
        self.freed.iter().copied().find_map(at).map(|r| std::format!("freed:{r}"))
    }

    fn new(files: Vec<(String, Vec<u8>)>, device: u32) -> Card {
        Card {
            files: files.into_iter().map(|(n, b)| (n.into_bytes(), b)).collect(),
            staged: Vec::new(),
            staged_path: Vec::new(),
            live: Vec::new(),
            allocs: 0,
            freed: Vec::new(),
            device,
            init_calls: Vec::new(),
            real_rows: false,
            skew_kind_handler: false,
            skew_patch_len: false,
            hooks: Vec::new(),
            provider_recs: Vec::new(),
            newer_parts: false,
            wrong_name: false,
            imports: crate::fake_platform::address_map::AddressMap::new(),
        }
    }
}

impl Images for Card {
    unsafe fn stage(&mut self, path: *const u8) -> Result<Staged, i32> {
        let key = core::slice::from_raw_parts(path, cstr_len(path, 64)).to_vec();
        match self.files.iter().find(|(n, _)| *n == key) {
            None => Err(E_NOT_FOUND),
            Some((_, bytes)) => {
                self.staged = bytes.clone();
                self.staged_path = key;
                Ok(Staged { ptr: self.staged.as_ptr(), size: self.staged.len() as u32 })
            }
        }
    }
    unsafe fn alloc(&mut self, n: u32) -> Option<Block> {
        self.allocs += 1;
        let addr = 0x0010_0000 * self.allocs;
        let mut mem = std::vec![0u128; (n as usize + 15) / 16];
        let ptr = mem.as_mut_ptr() as *mut u8;
        self.live.push((addr, mem));
        Some(Block { ptr, addr: dkrt_abi::dev_addr::DevAddr::new(addr) })
    }
    unsafe fn free(&mut self, addr: dkrt_abi::dev_addr::DevAddr) {
        let addr = addr.get();
        let at = self.live.iter().position(|b| b.0 == addr).expect("free of a block that is not live");
        let (_, mem) = self.live.remove(at);
        self.freed.push((addr, mem.len() as u32 * 16));
    }
    unsafe fn import_addr(&mut self, name: *const u8) -> u32 {
        // The device table `IMPORTS` points at firmware symbols that a host test binary does not link: a name the SDK declares gets a stand-in address.
        let n = core::slice::from_raw_parts(name, cstr_len(name, 64));
        // An import that another module provides (`requires` of its `[[import]]`) is optional on the device: it binds to the absent stub when the provider is not in the
        // image, so it gets a stand-in here too. A name that is neither is unknown and stays 0.
        let provided = core::str::from_utf8(n).ok().is_some_and(|n| adapter_of(n).is_some());
        if provided || dkrt_sdk::api::BUILTIN_NAMES.iter().any(|b| b.as_bytes() == n) {
            0x1000
        } else {
            0
        }
    }
    unsafe fn svc_import_addr(&mut self, name: *const u8) -> u32 {
        self.imports.with_addresses(|| RealImages.svc_import_addr(name))
    }
    unsafe fn import_needs(&mut self, name: *const u8) -> Option<&'static str> {
        let _ = name;
        None
    }
    unsafe fn list(&mut self, prefix: *const u8, out: *mut DkrtStoreEntry, max: u32) -> i32 {
        let prefix = core::slice::from_raw_parts(prefix, cstr_len(prefix, 64)).to_vec();
        let mut k = 0;
        for (name, bytes) in &self.files {
            if k < max as usize && name.starts_with(&prefix) {
                let e = &mut *out.add(k);
                e.path = [0; 64];
                e.path[..name.len()].copy_from_slice(name);
                e.size = bytes.len() as u32;
                k += 1;
            }
        }
        k as i32
    }
    unsafe fn enter_init(&mut self, rec: usize, init: u32, _: *const Ctx) -> i32 {
        self.hooks.push((false, rec));
        let f = &self.staged;
        if be(f, dkneeds::H_ROLE) != 1 {
            // a module's `init` is module code that the host does not run, with two exceptions: the code `moveq #n,%d0; rts` (what `init` returns at once, the one
            // instruction pair the host decodes) and the builder calls a case lists (`Card::init_calls`)
            let at = (be(f, dkneeds::H_IMAGE_OFF) + be(f, dkneeds::H_INIT)) as usize;
            if f[at] == 0x70 && f[at + 2..at + 4] == [0x4e, 0x75] {
                return f[at + 1] as i8 as i32;
            }
            // the calls are the module's own: they name its context while its `init` runs
            let prev = crate::dlog::enter_module(rec, crate::dlog::PHASE_INIT);
            let ctx = core::ptr::addr_of_mut!(crate::package::entry::DKRT_CTXS[rec]) as *mut u8; // not `ctx_arg`: the loader's host seam narrows it to 32 bits
            for call in &self.init_calls {
                match *call {
                    Builder::Machine(i) => crate::package::handlers::machines_fixture::add_machine(ctx, i),
                    Builder::Fx(i) => crate::package::handlers::fx_fixture::add_fx(ctx, i),
                    Builder::Workbench { offset, len } => {
                        let bytes = &f[offset..offset + len];
                        let status = crate::package::module_ui::dk_workbench_register(ctx, bytes.as_ptr(), len as u32);
                        if status != OK {
                            crate::dlog::leave_module(prev);
                            return status;
                        }
                    }
                }
            }
            crate::dlog::leave_module(prev);
            return OK;
        }
        let table = table_of(f);
        let keys = crate::package::collect::KeysCopy::from_bytes(&table).unwrap();
        let base = init - be(f, dkneeds::H_INIT);
        let at = |off: u32| (base + off) as usize as *const u8;
        let ctx = core::ptr::addr_of_mut!(crate::package::entry::DKRT_CTXS[rec]) as *mut u8;
        let mut kinds: Vec<(u32, u32)> = Vec::new();
        // the init of a service declares the hooks that its device has, with the bare entry: the packed table lists the hooks of every device, with a device mask in the entry
        let port = self.files.iter().find(|(n, _)| n.starts_with(b"ports/")).map(|(_, f)| table_of(f));
        let port = port.as_ref().map(|t| Keys::parse(t).unwrap());
        for i in 0..keys.decl.n {
            let mut d = keys.decl.items[i];
            if d.tag == Declare::HOOK {
                if !dkneeds::keys::hook_for_device(d.b, self.device) {
                    continue;
                }
                d.b &= dkneeds::keys::HOOK_ENTRY_MASK;
                if port.as_ref().is_some_and(|p| p.find_site(d.a).is_some_and(|x| x.flags & dkneeds::keys::FLAG_ABSENT != 0)) {
                    continue;
                }
            }
            // and the slots of its device, with the flags of the file's item without the device mask
            if d.tag == Declare::SLOT {
                if !dkneeds::keys::slot_for_device(d.b, self.device) {
                    continue;
                }
                d.b &= !dkneeds::keys::SLOT_DEVICES_MASK;
            }
            let r = if d.tag == Declare::KIND {
                kinds.push((d.a, d.b));
                crate::package::collect::dk_service_declare_kind(ctx, d.a, d.b, at(d.c + if self.skew_kind_handler { 4 } else { 0 }))
            } else if d.tag == Declare::SLOT {
                crate::package::collect::dk_service_declare_slot(ctx, d.a, d.b, at(d.c))
            } else if d.tag == Declare::HOOK && dkneeds::keys::hook_is_patch(d.b) {
                // a patch: `init` declares the bytes `start` computed (the fixture `local-patch` gives `PATCH_TEST_VALUE`) with the length of the port file's site
                let len = port.as_ref().and_then(|p| p.find_site(d.a)).map_or(2, |s| s.len as u32);
                crate::package::collect::dk_service_declare_patch(ctx, d.a, PATCH_TEST_VALUE, if self.skew_patch_len { len * 2 } else { len })
            } else if d.tag == Declare::HOOK {
                crate::package::collect::dk_service_declare_hook(ctx, d.a, at(d.b))
            } else if d.tag == Declare::EXPORT {
                crate::package::collect::dk_service_declare_export(ctx, d.a, at(d.b & !dkneeds::keys::EXPORT_FLAGS))
            } else {
                crate::package::collect::dk_service_declare_resource(ctx, d.a, d.b)
            };
            assert_eq!(r, OK);
        }
        // the row: the name of the file, the kinds of its table
        let stem = String::from_utf8(self.staged_path.clone()).unwrap();
        let stem = stem.trim_start_matches("services/").trim_end_matches(".dks").to_string();
        if stem == "local-ping" {
            // the file provides a `ProviderRow` over an owner object: the fixture's own row and instance (`dk_svc_start` is what its `init` does before the declarations)
            // a card load is a fresh file: an instance that an earlier case left (a boot that ended without the unload) is not this file's
            dkrt_fixture_local_ping::dk_svc_stop();
            let instance = dkrt_fixture_local_ping::dk_svc_start().expect("the instance of local-ping starts");
            self.provider_recs.push(rec);
            let row = if self.wrong_name { other_name_ping_row() } else if self.newer_parts { newer_ping_row() } else { dkrt_fixture_local_ping::dk_svc_row() };
            return crate::package::collect::dk_provider_declare_row(ctx, row, instance);
        }
        let name: &'static str = Box::leak(stem.into_boxed_str());
        let kinds: &'static [(u32, u32)] = Box::leak(kinds.into_boxed_slice());
        if self.real_rows && name == "fx" {
            // the fx file provides a `ProviderRow` over its owner: the file's own row and an instance (what its `init` makes before the declarations)
            crate::package::handlers::dks_decls::fx_stop();
            let instance = crate::package::handlers::dks_decls::fx_start().expect("the owner of the fx file starts");
            self.provider_recs.push(rec);
            return crate::package::collect::dk_provider_declare_row(ctx, crate::package::handlers::dks_decls::fx_row(), instance);
        }
        if self.real_rows && name == "machines" {
            // the machines file provides a `ProviderRow` over its owner, as the fx file does
            let instance = crate::package::handlers::dks_decls::machines_start();
            self.provider_recs.push(rec);
            return crate::package::collect::dk_provider_declare_row(ctx, crate::package::handlers::dks_decls::machines_file_row(), instance);
        }
        // any other file: a provider that owns the kinds of its table and does nothing with them
        crate::package::collect::dk_provider_declare_row(ctx, crate::package::fixture_provider::Nothing::owning(name, kinds).row(), crate::package::fixture_provider::instance())
    }
    fn image_extent(&mut self, _: u32, _: u32) -> core::ops::Range<usize> {
        // the rows of a host test are statics of the test binary, in no package image
        0..usize::MAX
    }
    unsafe fn enter_fini(&mut self, rec: usize, _: u32, _: *const Ctx) {
        self.hooks.push((true, rec));
        if let Some(at) = self.provider_recs.iter().position(|r| *r == rec) {
            self.provider_recs.remove(at);
            dkrt_fixture_local_ping::dk_svc_stop();
            crate::package::handlers::dks_decls::fx_stop();
            crate::package::handlers::dks_decls::machines_stop();
        }
    }
}

pub(super) type Boot = Loader<FakePlatform, Card, DeviceRegistry>;

/// The runtime tables of the host tests as a device has them before the boot loads anything: no loaded service (the other tests of the runtime run with the
/// machines row of `provider_rows::test_machines`; it is put back when the guard goes), no module, no DSP load.
pub(super) struct Clean(std::sync::MutexGuard<'static, ()>);

pub(super) fn clean() -> Clean {
    let g = LOCK.lock_with_package().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _ = crate::package::handlers::services(); // the first call adds the test row
        for rec in MAX_MODULES..MAX_MODULES + PROVIDER_MAX {
            crate::package::provider_rows::remove(rec);
        }
        *st() = core::mem::zeroed();
        records::reset();
        *dsp::ds() = core::mem::zeroed();
        dsp::init_once();
        CALLED_NEED = [0; NEEDS_LEN];
        // a case that ended early left the instance of the provider fixture started
        dkrt_fixture_local_ping::dk_svc_stop();
        crate::package::handlers::dks_decls::fx_stop();
        crate::package::handlers::dks_decls::machines_stop();
        // the link script is a static: a test with no DSP part never calls `script_link`, and would see the strict tick drop of the initial value or the state of
        // whichever test ran before it
        crate::package::cleanup_tests::lenient_tick_drop();
    }
    Clean(g)
}

impl Drop for Clean {
    fn drop(&mut self) {
        unsafe {
            for rec in MAX_MODULES..MAX_MODULES + PROVIDER_MAX {
                crate::package::provider_rows::remove(rec);
            }
            crate::package::provider_rows::add(MAX_MODULES + PROVIDER_MAX - 1, crate::package::handlers::test_seams::machines_row(), Some(200));
        }
    }
}

/// Where a fixture's source module is built (`<project id>:pack`) and its file name.
pub(super) struct Fixture {
    pub(super) leaf: &'static str,
    pub(super) nx: &'static str,
}

pub(super) const SINE: Fixture = Fixture { leaf: "sine", nx: "test/sine" };
const DYN_DN2: Fixture = Fixture { leaf: "dyn-dn2", nx: "test/dyn-dn2" };
pub(super) const FX_GAIN: Fixture = Fixture { leaf: "dyn-fx-gain", nx: "test/dyn-fx-gain" };
pub(super) const LFO_SHAPE: Fixture = Fixture { leaf: "lfo-shape", nx: "test/lfo-shape" };
/// A second shape under another module id: another shape key.
const LFO_SHAPE_B: Fixture = Fixture { leaf: "lfo-shape-b", nx: "test/lfo-shape-b" };
pub(super) const SVC_USER: Fixture = Fixture { leaf: "svc-test-user", nx: "test/svc-test-user" };
/// The test service with ColdFire hooks in the port file of the test image, and its user.
/// A service that takes one patch of the firmware (two bytes, a dead size bound of the chooser's order vector copy) and the module that needs it.
pub(super) const LOCAL_PATCH: Fixture = Fixture { leaf: "local-patch", nx: "test/local-patch" };
pub(super) const LOCAL_PATCH_USER: Fixture = Fixture { leaf: "local-patch-user", nx: "test/local-patch-user" };
pub(super) const SVC_TEST: Fixture = Fixture { leaf: "svc-test", nx: "test/svc-test" };
pub(super) const LOCAL_PING: Fixture = Fixture { leaf: "local-ping", nx: "test/local-ping" };
pub(super) const LOCAL_PING_USER: Fixture = Fixture { leaf: "local-ping-user", nx: "test/local-ping-user" };
pub(super) const LOCAL_PING_USER_B: Fixture = Fixture { leaf: "local-ping-user-b", nx: "test/local-ping-user-b" };
/// A module that binds a key combination with `dk_shortcut_add`: a user of the shortcuts service file, no DSP section.
pub(super) const SHORTCUT_DEMO: Fixture = Fixture { leaf: "shortcut-demo", nx: "test/shortcut-demo" };
/// A module that calls `dk_sample_pool_open` and the pool calls without owning a table: a user of the samples service file, no DSP section.
const TRACK_SELECT: Fixture = Fixture { leaf: "track-select", nx: "test/track-select" };

/// A module that keeps one setting and calls `dk_project_status`: a user of the project service file, no DSP section; it knows nothing of projects.
pub(super) const PROJECT_USER: Fixture = Fixture { leaf: "project-user", nx: "test/project-user" };

/// The service files of the card: the services of the runtime and the test service.
pub(super) const SERVICES: [Fixture; 7] = [
    Fixture { leaf: "fx", nx: "core/services/fx" },
    Fixture { leaf: "machines", nx: "core/services/machines" },
    Fixture { leaf: "lfo", nx: "core/services/lfo" },
    Fixture { leaf: "samples", nx: "core/services/samples" },
    Fixture { leaf: "svc-test", nx: "test/svc-test" },
    Fixture { leaf: "shortcuts", nx: "core/services/shortcuts" },
    Fixture { leaf: "project", nx: "core/services/project" },
];

fn card_of(dev: Dev, modules: &[&Fixture], services: &[Fixture]) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<(String, Vec<u8>)> =
        modules.iter().map(|m| (std::format!("modules/{}.dkm", m.leaf), packed(dev, m.leaf, "dkm", m.nx))).collect();
    for s in services {
        files.push((std::format!("services/{}.dks", s.leaf), service_file(dev, s.leaf, s.nx)));
    }
    files.push((std::format!("ports/{}.dkport", dev.port()), port_file(dev)));
    files
}

/// What `planned` leaves: the number of modules of the selection and of providers the search found.
pub(super) struct Plan {
    pub n: usize,
    pub np: usize,
}

/// The selection `modules` of a boot from a card with `services`, over the platform of `image`: the loader with every module captured and the providers the search found for their
/// hard needs. Nothing is loaded yet.
pub(super) unsafe fn planned(dev: Dev, image: &Image, limits: &[(u8, u32)], modules: &[&Fixture], services: &[Fixture]) -> (Boot, Plan) {
    let files = card_of(dev, modules, services);
    let p = image.platform(dev, limits);
    // the stock bytes of every hook site are live in the firmware (the check values of the port file are of them)
    for (addr, bytes) in stock_sites(dev).values() {
        p.put_code(*addr, bytes);
    }
    let mut l = Loader::new(p, Card::new(files, dev.profile.id), DeviceRegistry { runtime: package_object() });
    l.reset_plan();
    for (k, m) in modules.iter().enumerate() {
        l.capture(k, &packed(dev, m.leaf, "dkm", m.nx));
    }
    l.find_providers(modules.len());
    let np = l.found.n;
    (l, Plan { n: modules.len(), np })
}

/// A boot of the selection through the package manager over the real seams: what it did, and the means to ask about the packages afterwards.
pub(super) struct Run {
    pub pm: Manager<FakePlatform, Card, DeviceRegistry>,
    pub report: Report,
    s: Box<Session<FakePlatform, Card, DeviceRegistry>>,
    _paths: Vec<Vec<u8>>,
}

impl Run {
    /// The verdict the module states show for selection index `k`: the graph's or the rules of the core's refusal with its reason, else accepted (the load can still have failed
    /// for a reason of its own: `refusal`).
    pub fn verdict(&self, k: usize) -> Verdict {
        let id = PackageId(k as u8);
        match self.refusal(k) {
            None => Verdict::Accepted,
            Some(why) => {
                let service = if let LifecycleError::NeedRefused { service, .. } = why { service } else { id };
                let hash = self.pm.graph().node(service).and_then(|n| n.provides()).map_or(0, |a| a.name());
                crate::package::lifecycle::verdict_of(&why, self.s.reason(id), hash)
            }
        }
    }

    /// Why the manager did not bring up the module at selection index `k`, if it did not.
    pub fn refusal(&self, k: usize) -> Option<LifecycleError> {
        let id = PackageId(k as u8);
        self.report.refused().iter().find(|r| r.package == id).map(|r| r.reason)
    }

    /// The load status of the refusal of module `k`: `OK` when it was brought up.
    pub fn code(&self, k: usize) -> i32 {
        self.refusal(k).map_or(OK, |why| crate::package::lifecycle::code_of(&why))
    }

    pub fn is_up(&self, id: PackageId) -> bool {
        self.pm.live().contains(id)
    }
}

/// The live capacity of the platform of `l` with `own` given back (`loader::live_capacity` over the capacity of the fake platform; `(0, 0)`: what the live check counts).
pub(super) unsafe fn live_cap_with(l: &Boot, own: (u32, u32)) -> Vec<DkrtCap> {
    let mut caps = [DkrtCap { kind: 0, limit: 0, used: 0, largest: 0 }; crate::package::loader::CAP_MAX];
    let c = capacity(&l.p);
    let n = crate::package::loader::live_capacity(&mut caps, own, |out| {
        out[..c.len()].copy_from_slice(&c);
        c.len()
    });
    caps[..n].to_vec()
}

pub(super) unsafe fn live_cap(l: &Boot) -> Vec<DkrtCap> {
    live_cap_with(l, (0, 0))
}

/// The boot record of the selection `modules` (`modules/<leaf>.dkm`), as `dkrt_boot` leaves it before the load: empty, with the count.
pub(super) unsafe fn boot_record(leaves: &[&str]) -> dkrt_sdk::sel::Record {
    let mut rec = dkrt_sdk::sel::Record::empty();
    rec.count = leaves.len() as u32;
    for (k, leaf) in leaves.iter().enumerate() {
        let path = std::format!("modules/{leaf}.dkm");
        rec.paths[k][..path.len()].copy_from_slice(path.as_bytes());
    }
    *crate::boot::info() = core::mem::zeroed();
    crate::boot::info().count = leaves.len() as u32;
    crate::boot::states().begin();
    rec
}

/// Boot `modules` (the first `plan.n` of the selection) through the package manager over `l`, with the capacity of its platform. With `pin_services` the providers the search found
/// are roots too, so they stay up whatever happens to a module that the host cannot load.
pub(super) unsafe fn boot_run(l: &mut Boot, plan: &Plan, modules: &[&Fixture], pin_services: bool) -> Run {
    let paths: Vec<Vec<u8>> = modules.iter().map(|m| std::format!("modules/{}.dkm\0", m.leaf).into_bytes()).collect();
    let mut at = [None; MAX_MODULES];
    for (k, p) in paths.iter().enumerate() {
        at[k] = Some(p.as_ptr());
    }
    let mut s = Box::new(Session::new(l as *mut _, at));
    s.begin_boot(&capacity(&l.p), modules.len());
    let manifest = RtManifest::of(&mut *s, modules.len());
    let (placer, linker, installers, host) = crate::package::lifecycle::traced_seams(&mut *s);
    let mut pm = Manager::new(manifest, placer, linker, installers, host, 50).unwrap();
    let mut roots: Vec<PackageId> = (0..modules.len()).map(|k| PackageId(k as u8)).collect();
    if pin_services {
        roots.extend((0..plan.np).map(|j| PackageId((MAX_MODULES + j) as u8)));
    }
    l.phase = Phase::Boot;
    let report = pm.boot(&roots).unwrap();
    l.phase = Phase::Closed;
    crate::package::boot_idx::set(u32::MAX);
    Run { pm, report, s, _paths: paths }
}

/// The store paths of the providers the search found.
fn found(l: &Boot, plan: &Plan) -> BTreeSet<String> {
    (0..plan.np).map(|j| String::from_utf8(l.found.path(j).to_vec()).unwrap()).collect()
}

/// The hook sites (hook name hash, address) that the service files which loaded install: each `HOOKS` item of the packed file that the port file lists as present.
fn binds_of(dev: Dev, l: &Boot, plan: &Plan) -> Vec<(u32, u32)> {
    binds_split(dev, l, plan, false)
}

/// The patch sites of the service files that came up (the `HOOKS` items with the patch entry): (name hash, address).
fn patches_of(dev: Dev, l: &Boot, plan: &Plan) -> Vec<(u32, u32)> {
    binds_split(dev, l, plan, true)
}

/// The sites of the `HOOKS` items of the service files that came up: the hooks, or (`patches`) the patches.
fn binds_split(dev: Dev, l: &Boot, plan: &Plan, patches: bool) -> Vec<(u32, u32)> {
    let pf = port_file(dev);
    let pt = table_of(&pf);
    let port = Keys::parse(&pt).unwrap();
    let mut out = Vec::new();
    for j in 0..plan.np {
        let path = String::from_utf8(l.found.path(j).to_vec()).unwrap();
        if !path.starts_with("services/") || unsafe { records::module(MAX_MODULES + j).used } == 0 {
            continue;
        }
        let (_, file) = l.m.files.iter().find(|(n, _)| *n == path.as_bytes()).unwrap();
        let t = table_of(file);
        let keys = Keys::parse(&t).unwrap();
        for i in 0..keys.count(tag::HOOKS) {
            let (hash, entry, _) = keys.words(tag::HOOKS, i).unwrap();
            // the entry of another device (one name may have an entry for each device)
            if !dkneeds::keys::hook_for_device(entry, dev.profile.id) || dkneeds::keys::hook_is_patch(entry) != patches {
                continue;
            }
            let site = port.find_site(hash).expect("the port file lists the hook");
            if site.flags & dkneeds::keys::FLAG_ABSENT == 0 {
                out.push((hash, site.addr));
            }
        }
    }
    out
}

/// Every name the file imports resolves: against the runtime's tables (and the service-only ones for a service file) or an export of a loaded provider. The loader
/// does the same at a load; a module with a DSP section is not loaded on the host, so its imports are looked at here.
unsafe fn imports_resolve(l: &mut Boot, image: &Image, file: &[u8], what: &str) -> bool {
    let mut adapter_imports = false;
    let role = be(file, dkneeds::H_ROLE);
    let (off, n) = (be(file, dkneeds::H_IMPORT_OFF) as usize, be(file, dkneeds::H_IMPORT_N) as usize);
    for i in 0..n {
        let at = off + i * dkneeds::CF_IMPORT_LEN as usize;
        let name = &file[at..at + dkneeds::CF_IMPORT_LEN as usize];
        let len = name.iter().position(|c| *c == 0).expect("an import name without NUL");
        // the loader's stand-in for an import another module provides is not an answer here: this check asks whether the image has that module
        let provided = adapter_of(std::str::from_utf8(&name[..len]).unwrap()).is_some();
        let mut a = if provided { 0 } else { l.m.import_addr(name.as_ptr()) };
        if a == 0 && role == 1 {
            a = l.m.svc_import_addr(name.as_ptr());
        }
        if a == 0 {
            // the export of a loaded provider, bound to the consumers that need it
            a = l.exports.owned(name_hash(std::str::from_utf8(&name[..len]).unwrap())).map_or(0, |e| e.0);
        }
        // a service of an image adapter: the host cannot link it, so it is accepted when the image set has the adapter (the module then loads there, and its `_open`
        // answers DK_E_MISSING on a device without it); an image without the adapter is no place for the module
        let adapter = if a == 0 { adapter_of(std::str::from_utf8(&name[..len]).unwrap()) } else { None };
        if let Some(m) = adapter {
            if image.modules.iter().any(|r| *r == m) {
                adapter_imports = true;
                continue;
            }
        }
        assert_ne!(a, 0, "{what} imports {}, which the runtime and the loaded services do not have", String::from_utf8_lossy(&name[..len]));
    }
    adapter_imports
}

/// The `dyndsp_info` table of the host tests for the providers of `plan` that have a DSP part: the device word of their DSP header, a wide area and one import per
/// `DSP_IMPORT` line of the dyndsp source of the device. `None` when no provider has a DSP part.
fn dsp_info_of(dev: Dev, l: &Boot, plan: &Plan) -> Option<Vec<u32>> {
    let mut device = None;
    for j in 0..plan.np {
        let path = l.found.path(j);
        let (_, file) = l.m.files.iter().find(|(n, _)| *n == path).unwrap();
        if be(file, dkneeds::H_DSP_SIZE) != 0 {
            device = Some(be(file, be(file, dkneeds::H_DSP_OFF) + dkneeds::DSP_H_DEVICE));
        }
    }
    let device = device?;
    let src = std::fs::read_to_string(repo().join(std::format!("runtime/platform/dyndsp/src/cf/dyndsp-{}.s", dev.name))).unwrap();
    let names: Vec<&str> = src.lines().filter_map(|t| t.trim().strip_prefix("DSP_IMPORT ")).map(str::trim).collect();
    let mut t = vec![0u32; dkrt_abi::dsp::I_IMPORTS];
    t[dkrt_abi::dsp::I_MAGIC] = dkrt_abi::dsp::INFO_MAGIC;
    t[dkrt_abi::dsp::I_DEVICE] = device;
    t[dkrt_abi::dsp::I_AREA] = 0x2000_0000;
    t[dkrt_abi::dsp::I_AREA_BYTES] = 1 << 20;
    t[dkrt_abi::dsp::I_VFIRST] = 4;
    t[dkrt_abi::dsp::I_VFREE] = 8;
    t[dkrt_abi::dsp::I_NIMPORT] = names.len() as u32;
    for (i, n) in names.iter().enumerate() {
        // an entry is a NUL padded name and, at `DSP_IMPORT_ADDR`, the address as a host word (the runtime reads the table in place)
        let mut e = [0u8; dkneeds::DSP_IMPORT_LEN as usize];
        e[..n.len()].copy_from_slice(n.as_bytes());
        let mut words: Vec<u32> = e.chunks(4).map(|c| u32::from_ne_bytes(c.try_into().unwrap())).collect();
        words[dkneeds::DSP_IMPORT_ADDR as usize / 4] = 0x1000 + 4 * i as u32;
        t.extend_from_slice(&words);
    }
    Some(t)
}

/// The DSP of a provider's load for a host test (see `dsp_info_of`): the table is in place and the link acknowledges while this lives.
pub(super) struct DspEnv {
    // fields drop in this order: the script ends first, the table it reads goes last
    _script: crate::package::cleanup_tests::LinkScript,
    _guard: crate::package::cleanup_tests::VoiceInfoGuard,
    _info: Vec<u32>,
}

pub(super) unsafe fn dsp_env(dev: Dev, l: &Boot, plan: &Plan) -> Option<DspEnv> {
    let info = dsp_info_of(dev, l, plan)?;
    let guard = crate::package::cleanup_tests::VoiceInfoGuard::set(info.as_ptr());
    let script = crate::package::cleanup_tests::LinkScript::new(true);
    Some(DspEnv { _script: script, _guard: guard, _info: info })
}

/// The seam of a fixture with its provider: the graph and the rules of the core accept the module and the search finds `want` on the card, the package manager brings the providers up (the
/// ABI word, the stock bytes of every hook site), their hooks and slots are installed as their bind records say, and a module without a DSP section loads.
unsafe fn fits(dev: Dev, set: &str, modules: &[&Fixture], want: &[&str]) {
    let image = Image::read(dev, set);
    let (mut l, plan) = planned(dev, &image, &[], modules, &SERVICES);
    let what = std::format!("{} {set} {:?}", dev.name, modules.iter().map(|m| m.leaf).collect::<Vec<_>>());
    let got = found(&l, &plan);
    for w in want {
        assert!(got.contains(&std::format!("services/{w}.dks")), "{what}: the search found {got:?}, not {w}");
    }
    // a provider with a DSP part (the machines service) reaches the DSP loader from its load: the host gives it the import table of `core/dyndsp` (every name of its
    // sources, an address that is not 0) and a link that acknowledges, and stops there (the DSP code and the slots follow in the UI loop, `dsp_provider_tests`)
    let _dsp = dsp_env(dev, &l, &plan);
    // the providers are roots too: they stay up whatever happens to a module that the host cannot load
    let run = boot_run(&mut l, &plan, modules, true);
    for k in 0..modules.len() {
        assert_eq!(run.verdict(k), Verdict::Accepted, "{what}: {}: {:?} missing {:?} import_hash {:x} found {:?}", modules[k].leaf, run.report.refused(), String::from_utf8_lossy(&l.missing), l.import_hash, found(&l, &plan));
    }
    for j in 0..plan.np {
        assert!(
            run.is_up(PackageId((MAX_MODULES + j) as u8)),
            "{what}: provider {} did not load: {:?}",
            String::from_utf8_lossy(l.found.path(j)),
            run.report.refused().iter().map(|r| r.reason).collect::<Vec<_>>()
        );
    }
    // the rows the files declared are in the table of the services
    for w in want {
        assert!(crate::package::provider_rows::has(w), "{what}: no row {w} in the table");
    }
    // the hook sites of the files: one `jmp` written for each hook of a service file that the port lists as present, at its address, under one mask
    let binds = binds_of(dev, &l, &plan);
    let patches = patches_of(dev, &l, &plan);
    assert_eq!(l.installer.count(), binds.len(), "{what}: installed sites");
    assert_eq!(l.installer.patch_count(), patches.len(), "{what}: installed patches");
    let written: Vec<u32> = l.p.calls.borrow().iter().filter_map(|c| if let Call::Write { addr, .. } = c { Some(*addr) } else { None }).collect();
    let mut sites: Vec<u32> = binds.iter().chain(patches.iter()).map(|b| b.1).collect();
    sites.sort();
    let mut w2 = written.clone();
    w2.sort();
    assert_eq!(w2, sites, "{what}: the sites written are the bind records'");
    // each module: its imports resolve, and without a DSP section it loads
    for (k, m) in modules.iter().enumerate() {
        let file = packed(dev, m.leaf, "dkm", m.nx);
        // a module with a service of an image adapter among its imports (core/ui) is not loaded here: its link needs the adapter
        let adapter_imports = imports_resolve(&mut l, &image, &file, m.leaf);
        if be(&file, dkneeds::H_DSP_SIZE) == 0 && !adapter_imports {
            assert!(run.is_up(PackageId(k as u8)), "{what}: load of {} (module {k}): {:?}", m.leaf, run.refusal(k));
        }
    }
}

// ---- the cases ------------------------------------------------------------------------------------------------------------

#[test]
fn a_selected_consumer_loads_a_service_declared_outside_the_sdk() {
    let _g = clean();
    for dev in DEVICES.iter().copied().filter(Dev::has_firmware) {
        let image = Image::read(dev, "emulator");
        unsafe {
            let (l, plan) = planned(dev, &image, &[], &[], &[LOCAL_PING]);
            assert!(found(&l, &plan).is_empty(), "an unneeded provider on the card is not loaded");

            let (mut l, plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[]);
            assert!(found(&l, &plan).is_empty());
            let service = name_hash("local-ping");
            assert_eq!(boot_run(&mut l, &plan, &[&LOCAL_PING_USER], false).verdict(0), Verdict::Refused(Reason::NeedMissing(service)));

            let (mut l, plan) = planned(dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            assert_eq!(found(&l, &plan), BTreeSet::from(["services/local-ping.dks".into()]));

            let mut newer_client = packed(dev, LOCAL_PING_USER.leaf, "dkm", LOCAL_PING_USER.nx);
            let table = table_of(&newer_client);
            let at = table.windows(8).position(|w| w[..4] == service.to_be_bytes() && w[4..] == 0x010200u32.to_be_bytes()).unwrap()
                + be(&newer_client, dkneeds::H_REG_OFF) as usize + 4;
            newer_client[at..at + 4].copy_from_slice(&0x010201u32.to_be_bytes());
            seal(&mut newer_client);
            l.capture(0, &newer_client);
            let too_new = boot_run(&mut l, &plan, &[&LOCAL_PING_USER], false);
            assert_eq!(too_new.verdict(0), Verdict::Refused(Reason::NeedVersion));
            newer_client[at..at + 4].copy_from_slice(&0x000100u32.to_be_bytes());
            seal(&mut newer_client);
            l.capture(0, &newer_client);
            let different_major = boot_run(&mut l, &plan, &[&LOCAL_PING_USER], false);
            assert_eq!(different_major.verdict(0), Verdict::Refused(Reason::NeedVersion));
            // a module the graph refuses brings up none of its services
            assert!(!crate::package::provider_rows::has("local-ping"));

            l.capture(0, &packed(dev, LOCAL_PING_USER.leaf, "dkm", LOCAL_PING_USER.nx));
            let run = boot_run(&mut l, &plan, &[&LOCAL_PING_USER], false);
            assert_eq!(run.verdict(0), Verdict::Accepted);
            assert!(run.is_up(PackageId(MAX_MODULES as u8)));
            assert!(crate::package::provider_rows::has("local-ping"));
            assert!(run.is_up(PackageId(0)), "the consumer loads against the provider: {:?}", run.refusal(0));
        }
        clean_state();
    }
}

#[test]
fn a_machine_loads_with_the_machines_service_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        unsafe {
            fits(*dev, "emulator", &[&SINE], &["machines"]);
        }
        let _ = clean_state();
        if dev.name == "dn2" {
            unsafe {
                fits(*dev, "emulator", &[&DYN_DN2], &["machines"]);
            }
        }
        let _ = clean_state();
    }
}

#[test]
fn an_effect_loads_with_the_fx_service_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        unsafe {
            fits(*dev, "emulator", &[&FX_GAIN], &["fx"]);
        }
        let _ = clean_state();
    }
}

/// No image point holds a table for the fx service (RC4-2): the patched image of every set has no `fxpages_info`, only the `rows_info` the service finds its parameter blocks in (the
/// blocks themselves: `tests/rows.rs` of patchkit against the device files, `tests/table.rs` of the service), and the service file starts over it from its own device facts and loads an
/// effect (the facts of each device: `tests/table.rs` of the service).
#[test]
fn the_fx_service_file_loads_over_an_image_with_no_effect_pages_point() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for set in ["emulator", "device"] {
            let image = Image::read(*dev, set);
            let has = |name: &str| image.entries.iter().any(|e| e.1 == name_hash(name) && e.2 == 0);
            assert!(!has("fxpages_info"), "{} {set}: the image has a table for the effect pages", dev.name);
            assert!(has("rows_info"), "{} {set}: no rows_info", dev.name);
            unsafe {
                fits(*dev, set, &[&FX_GAIN], &["fx"]);
            }
            let _ = clean_state();
        }
    }
}

/// The six interrupt sites of the LFO tick per device and their stock bytes: `moveq 6,Dn; cmp.l D3,Dn; beq|bne` (finding 149).
const LFO_TICK_SITES: [(&str, &str, [u8; 6]); 12] = [
    ("dt2", "tick_call", [0x7e, 0x06, 0xbe, 0x83, 0x67, 0x28]),
    ("dt2", "tick_slew", [0x74, 0x06, 0xb4, 0x83, 0x66, 0x2c]),
    ("dt2", "tick_start", [0x70, 0x06, 0xb0, 0x83, 0x67, 0x16]),
    ("dt2", "read_call", [0x70, 0x06, 0xb0, 0x83, 0x67, 0x30]),
    ("dt2", "read_slew", [0x74, 0x06, 0xb4, 0x83, 0x66, 0x2a]),
    ("dt2", "read_start", [0x74, 0x06, 0xb4, 0x83, 0x67, 0x16]),
    ("dn2", "tick_call", [0x70, 0x06, 0xb0, 0x83, 0x67, 0x20]),
    ("dn2", "tick_slew", [0x72, 0x06, 0xb2, 0x83, 0x66, 0x2e]),
    ("dn2", "tick_start", [0x70, 0x06, 0xb0, 0x83, 0x67, 0x16]),
    ("dn2", "read_call", [0x70, 0x06, 0xb0, 0x83, 0x67, 0x30]),
    ("dn2", "read_slew", [0x74, 0x06, 0xb4, 0x83, 0x66, 0x2a]),
    ("dn2", "read_start", [0x74, 0x06, 0xb4, 0x83, 0x67, 0x16]),
];

#[test]
fn an_lfo_loads_with_the_lfo_service_file() {
    let _g = clean();
    // one file for every device: each device above loads the same bytes (the hooks of the other device carry a device mask that the loader skips)
    let files: Vec<Vec<u8>> = DEVICES.iter().map(|d| service_file(*d, "lfo", "core/services/lfo")).collect();
    assert!(files.len() > 1 && files.iter().all(|f| *f == files[0]), "lfo.dks differs between devices");
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        // there is no image half: `fits` loads the file and installs each hook it declares (the six interrupt hooks of the tick and the UI hooks) over the stock bytes
        unsafe {
            fits(*dev, "emulator", &[&LFO_SHAPE], &["lfo"]);
        }
        let _ = clean_state();
        // the hooks: the port file lists each hook the lfo service takes and the device has: the six interrupt sites of the LFO tick (`moveq 6,Dn; cmp.l D3,Dn; b<cc>`), the name
        // formatter and the page cell's vector pick (the Digitone II also its three-character formatter: `link.w A6,-0x1c; moveq 6,D0`), over the stock bytes of the firmware.
        // The check value of each is what the loader compares at the install.
        let file = service_file(*dev, "lfo", "core/services/lfo");
        let table = table_of(&file);
        let keys = Keys::parse(&table).unwrap();
        let pf = port_file(*dev);
        let pt = table_of(&pf);
        let port = Keys::parse(&pt).unwrap();
        let stock = stock_sites(*dev);
        let mut got = Vec::new();
        for i in 0..keys.count(tag::HOOKS) {
            let (hash, entry, _) = keys.words(tag::HOOKS, i).unwrap();
            if dkneeds::keys::hook_for_device(entry, dev.profile.id) && port.find_site(hash).is_some_and(|s| s.flags & dkneeds::keys::FLAG_ABSENT == 0) {
                got.push((hash, stock[&hash].1.clone()));
            }
        }
        let name = |h: &str| name_hash(std::format!("cf.lfo.hook.{h}").as_str());
        let mut want: Vec<(u32, Vec<u8>)> = LFO_TICK_SITES
            .iter()
            .filter(|s| s.0 == dev.name)
            .map(|s| (name(s.1), s.2.to_vec()))
            .collect();
        want.extend(std::vec![
            (name("wave_name"), std::vec![0x4e, 0x56, 0xff, 0xe4, 0x70, 0x06]),
            // the vector pick of the page's cell: `addi.l #TABLE,D6` with the base of the firmware's RAM table of tile vectors (the hook's tail replays it)
            (
                name("graph_vec"),
                [0x06u8, 0x86]
                    .into_iter()
                    .chain(if dev.name == "dn2" { 0x4450_8b68u32 } else { 0x44d2_e37cu32 }.to_be_bytes())
                    .collect(),
            ),
        ]);
        if dev.name == "dn2" {
            want.push((name("wave_name3"), std::vec![0x4e, 0x56, 0xff, 0xe4, 0x70, 0x06]));
        }
        got.sort();
        want.sort();
        assert_eq!(got, want, "{}: the sites of lfo.dks", dev.name);
        // the four firmware words of the WAVE maximum (`DKRT_LFO_FW`): the stock parameter table and the rows of the codes 79, 89 and 99, which the file imports from the port file
        // (`dk_fw_<hash>` of each name). A name the port lacks would refuse the file, and an absent one would make the service write nothing, so each must be an address;
        // the rows lie 60 bytes apart in the table
        let addr = |n: &str| match port.find_name(dkneeds::keys::NAME_FW, name_hash(n), 0) {
            Some(x) if x.flags & dkneeds::keys::FLAG_ABSENT == 0 => x.addr,
            other => panic!("{}: {n} is {other:?} in the port file", dev.name),
        };
        let words = [addr("cf.param.data.descriptor_table"), addr("cf.lfo.data.wave_row.1"), addr("cf.lfo.data.wave_row.2"), addr("cf.lfo.data.wave_row.3")];
        for (k, code) in [79u32, 89, 99].into_iter().enumerate() {
            assert_eq!(words[1 + k], words[0] + 60 * code, "{}: WAVE row {} is not table + 60 * {code}", dev.name, k + 1);
        }
    }
}

/// The six interrupt hooks of the LFO tick are `jmp` at their stock sites after the load, and the unload of the lfo service writes the stock bytes back (docs/plans/runtime-contract-and-lifecycle.md, RC4).
#[test]
fn the_lfo_tick_hooks_patch_their_stock_sites_and_the_unload_restores_them() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        // the port file of the first device is a loaded provider: each device starts from a clean state
        clean_state();
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let _dsp = dsp_env(*dev, &l, &plan);
            let mut run = boot_run(&mut l, &plan, &[&LFO_SHAPE], true);
            let lfo = lfo_package(&l, &plan);
            assert!(run.is_up(lfo), "{}: lfo.dks is up", dev.name);
            let stock = stock_sites(*dev);
            let sites: Vec<(u32, [u8; 6])> = LFO_TICK_SITES
                .iter()
                .filter(|s| s.0 == dev.name)
                .map(|s| (stock[&name_hash(std::format!("cf.lfo.hook.{}", s.1).as_str())].0, s.2))
                .collect();
            assert_eq!(sites.len(), 6, "{}", dev.name);
            let read = |l: &Boot, addr: u32| {
                let mut b = [0u8; 6];
                l.p.read_code(addr, &mut b);
                b
            };
            for (addr, bytes) in &sites {
                let live = read(&l, *addr);
                assert_eq!(live[..2], 0x4ef9u16.to_be_bytes(), "{}: {addr:#x} is not a jmp after the load", dev.name);
                assert_ne!(live, *bytes, "{}: {addr:#x} is still stock", dev.name);
            }
            run.pm.unload(lfo).unwrap();
            for (addr, bytes) in &sites {
                assert_eq!(read(&l, *addr), *bytes, "{}: {addr:#x} after the unload", dev.name);
            }
        }
    }
}

/// The package of `lfo.dks` among the providers the search found.
fn lfo_package(l: &Boot, plan: &Plan) -> PackageId {
    let j = (0..plan.np).find(|j| l.found.path(*j) == b"services/lfo.dks").expect("lfo.dks is found");
    PackageId((MAX_MODULES + j) as u8)
}

/// The hook sites of the lfo service hold their stock bytes (the live bytes of `planned`).
fn lfo_sites_are_stock(dev: Dev, l: &Boot) {
    let stock = stock_sites(dev);
    let mut n = 0;
    for s in LFO_TICK_SITES.iter().filter(|s| s.0 == dev.name) {
        let (addr, _) = stock[&name_hash(std::format!("cf.lfo.hook.{}", s.1).as_str())];
        let mut b = [0u8; 6];
        unsafe { l.p.read_code(addr, &mut b) };
        assert_eq!(b, s.2, "{}: {addr:#x} is not stock", dev.name);
        n += 1;
    }
    assert_eq!(n, 6, "{}", dev.name);
}

/// An `lfo.dks` built for another ABI is refused by its own load (`E_SVC_ABI`) before anything is allocated, installed or written.
#[test]
fn an_lfo_service_file_of_another_abi_is_refused_before_anything_is_written() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let lfo = lfo_package(&l, &plan);
            let at = l.m.files.iter().position(|(n, _)| n == b"services/lfo.dks").unwrap();
            let word = be(&l.m.files[at].1, dkneeds::H_SVC_ABI);
            assert_ne!(word, 0, "lfo.dks has an ABI word");
            put(&mut l.m.files[at].1, dkneeds::H_SVC_ABI, word ^ 1);
            seal(&mut l.m.files[at].1);
            let run = boot_run(&mut l, &plan, &[&LFO_SHAPE], true);
            let why = run.report.refused().iter().find(|r| r.package == lfo).expect("lfo.dks is refused").reason;
            assert_eq!(crate::package::lifecycle::code_of(&why), E_SVC_ABI, "{}", dev.name);
            assert_eq!(l.installer.count(), 0);
            lfo_sites_are_stock(*dev, &l);
        }
        let _ = clean_state();
    }
}

/// An `lfo.dks` whose `init` declares the `lfo_shape` handler at another place than its header table says is refused by the commit (`E_DECL_KIND`); the hook sites stay stock.
#[test]
fn an_lfo_init_that_declares_another_kind_handler_than_the_table_is_refused() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let lfo = lfo_package(&l, &plan);
            l.m.skew_kind_handler = true;
            let run = boot_run(&mut l, &plan, &[&LFO_SHAPE], true);
            let why = run.report.refused().iter().find(|r| r.package == lfo).expect("lfo.dks is refused").reason;
            assert_eq!(crate::package::lifecycle::code_of(&why), E_DECL_KIND, "{}", dev.name);
            assert_eq!(l.installer.count(), 0);
            lfo_sites_are_stock(*dev, &l);
        }
        let _ = clean_state();
    }
}

/// Two `lfo_shape` modules with different keys are both accepted (the refusal of one key twice is `of_two_lfo_shapes_with_one_key_the_first_loads_and_the_second_is_refused_by_the_boot` in `manager_assembly_tests.rs`).
#[test]
fn two_lfo_shapes_with_different_keys_are_both_accepted() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let key_of = |f: &Fixture| {
            let table = table_of(&packed(*dev, f.leaf, "dkm", f.nx));
            let key = Keys::parse(&table).unwrap().keys_of(dkneeds::KIND_LFO_SHAPE).next().expect("the fixture has a shape key");
            key
        };
        assert_ne!(key_of(&LFO_SHAPE), key_of(&LFO_SHAPE_B), "the second fixture has its own key");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE, &LFO_SHAPE_B], &SERVICES);
            let _dsp = dsp_env(*dev, &l, &plan);
            let run = boot_run(&mut l, &plan, &[&LFO_SHAPE, &LFO_SHAPE_B], false);
            assert_eq!(run.verdict(0), Verdict::Accepted, "{}", dev.name);
            assert_eq!(run.verdict(1), Verdict::Accepted, "{}", dev.name);
        }
        let _ = clean_state();
    }
}

/// A shape module on a card with no `lfo.dks` is refused naming the service (`E_MISSING`).
#[test]
fn an_lfo_shape_without_the_lfo_service_file_is_refused_naming_the_service() {
    let _g = clean();
    let others: Vec<Fixture> = SERVICES.iter().filter(|s| s.leaf != "lfo").map(|s| Fixture { leaf: s.leaf, nx: s.nx }).collect();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &others);
            let run = boot_run(&mut l, &plan, &[&LFO_SHAPE], false);
            assert_eq!(run.verdict(0), Verdict::Refused(Reason::NeedMissing(name_hash("lfo"))), "{}", dev.name);
            assert_eq!(run.code(0), E_MISSING);
        }
        let _ = clean_state();
    }
}

/// The reload seam (docs/plans/extension-model.md E5): a real packed module of the boot is checked against the live set, taken down and loaded again into its
/// record, twice; a service file at a module's path is refused by the role check before anything is unloaded.
#[test]
fn a_packed_module_is_reloaded_twice_through_the_plan_and_a_service_file_is_refused() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let _dsp = dsp_env(*dev, &l, &plan);
            let path = "modules/lfo-shape.dkm\0";
            let rec = boot_record(&["lfo-shape"]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            assert_eq!(crate::boot::info().results[0], OK, "{}", dev.name);
            crate::package::boot_idx::set(u32::MAX);
            l.phase = Phase::Closed;
            let mut caps = capacity(&l.p);
            crate::package::loader::live_caps(&mut caps, crate::package::loader::own_use(0));
            // a service file is no module: refused, and the module is as it was
            assert_eq!(l.reload_check(0, "services/lfo.dks\0".as_ptr(), &caps), E_FORMAT, "{}", dev.name);
            assert_eq!(records::module(0).used, 1);
            for round in 0..2 {
                assert_eq!(l.reload_check(0, path.as_ptr(), &caps), OK, "{} round {round}", dev.name);
                assert_eq!(rt.unload(0), OK, "{} round {round}", dev.name);
                assert_eq!(rt.reload_load_with(0, path.as_ptr(), &live_cap(&l)), OK, "{} round {round}", dev.name);
                assert_eq!((records::module(0).used, l.sel_of(0)), (1, 0));
            }
        }
        let _ = clean_state();
    }
}

/// The seam of the FX ROUTING pick (E1): a pick saves `fx/route/<id>` and reloads the effect's module, so the real packed effect `dyn-fx-gain` must pass the reload check against the live
/// set (with the fx service up), be taken down and be loaded again into its record, twice. The host runs no module code, so what the load registers (the route read at `apply`) is the
/// service's unit test (`dkrt-svc-fx` `routing`) and the device test; the job's other refusals (a menu built for the module, DSP rows that stay held) are facts of the module, here none.
#[test]
fn a_packed_effect_module_is_reloaded_twice_through_the_plan() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES);
            // the host models no DSP rows and no effect registry: the file without its DSP part, as the boot-effects scenario takes it
            swap_module(&mut l, 0, FX_GAIN.leaf, without_dsp(*dev, &FX_GAIN));
            let _dsp = dsp_env(*dev, &l, &plan);
            let path = "modules/dyn-fx-gain.dkm\0";
            let rec = boot_record(&["dyn-fx-gain"]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            assert_eq!(crate::boot::info().results[0], OK, "{}", dev.name);
            crate::package::boot_idx::set(u32::MAX);
            l.phase = Phase::Closed;
            let mut caps = capacity(&l.p);
            crate::package::loader::live_caps(&mut caps, crate::package::loader::own_use(0));
            for round in 0..2 {
                assert_eq!(l.reload_check(0, path.as_ptr(), &caps), OK, "{} round {round}", dev.name);
                assert_eq!(rt.unload(0), OK, "{} round {round}", dev.name);
                assert_eq!(rt.reload_load_with(0, path.as_ptr(), &live_cap(&l)), OK, "{} round {round}", dev.name);
                assert_eq!((records::module(0).used, l.sel_of(0)), (1, 0), "{} round {round}", dev.name);
            }
        }
        let _ = clean_state();
    }
}

/// The live seam (docs/plans/extension-model.md E5c): a real packed module of the boot is taken down, its file is loaded again at run time under a new path at a new
/// selection index, taken down, and loaded again at its own path and index, each time through the file checks and the plan over the live set. A file whose name is loaded
/// is refused first.
#[test]
fn a_packed_module_is_unloaded_loaded_under_a_new_path_and_loaded_again_at_run_time() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let _dsp = dsp_env(*dev, &l, &plan);
            let (path, copy) = ("modules/lfo-shape.dkm\0", "modules/lfo-shape-copy.dkm\0");
            let bytes = l.m.files.iter().find(|(n, _)| n == b"modules/lfo-shape.dkm").expect("the module is on the card").1.clone();
            l.m.files.push((b"modules/lfo-shape-copy.dkm".to_vec(), bytes));
            let rec = boot_record(&["lfo-shape"]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            assert_eq!(crate::boot::info().results[0], OK, "{}", dev.name);
            crate::package::boot_idx::set(u32::MAX);
            l.phase = Phase::Closed;
            let caps = |l: &Boot| {
                let mut c = capacity(&l.p);
                crate::package::loader::live_caps(&mut c, (0, 0));
                c
            };
            // the module of that name is loaded: its copy is refused by name, before anything changes
            assert_eq!(l.live_check_load(1, copy.as_ptr(), &caps(&l)), E_LOADED, "{}", dev.name);
            assert_eq!((records::module(1).used, l.accepted[1]), (0, false));
            // an unload, then the copy at a new index (its file name was not in the boot's selection)
            assert_eq!(rt.unload(0), OK, "{}", dev.name);
            l.retire_index(0);
            assert_eq!((records::module(0).used, l.accepted[0], l.sel_of(0)), (0, false, u8::MAX), "{}", dev.name);
            assert_eq!(l.live_check_load(1, copy.as_ptr(), &caps(&l)), OK, "{}", dev.name);
            assert_eq!(rt.live_load_with(1, copy.as_ptr(), &live_cap(&l)), OK, "{}", dev.name);
            assert_eq!((records::module(1).used, l.sel_of(1), l.accepted[1]), (1, 1, true), "{}", dev.name);
            // and down again, then the first path at its index
            assert_eq!(rt.unload(1), OK, "{}", dev.name);
            l.retire_index(1);
            assert_eq!(l.live_check_load(0, path.as_ptr(), &caps(&l)), OK, "{}", dev.name);
            assert_eq!(rt.live_load_with(0, path.as_ptr(), &live_cap(&l)), OK, "{}", dev.name);
            assert_eq!((records::module(0).used, l.sel_of(0), l.accepted[0], l.accepted[1]), (1, 0, true, false), "{}", dev.name);
            assert!(rt.manager().unwrap().live().contains(PackageId(0)), "{}", dev.name);
        }
        let _ = clean_state();
    }
}

/// A module that uses the sample pool is found by its needs: the search finds `services/samples.dks`, the file loads (ABI word, declare check, exports and the
/// two pool slots as the bind records say), and the module's imports (`dk_sample_pool_*`) resolve to the file's exports.
#[test]
fn a_sample_pool_user_loads_with_the_samples_service_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        unsafe {
            fits(*dev, "emulator", &[&TRACK_SELECT], &["samples"]);
        }
        let _ = clean_state();
    }
}

/// The same module on a card without `samples.dks`: the plan refuses it (its need of the samples service is missing), nothing is loaded.
#[test]
fn a_sample_pool_user_is_refused_without_the_samples_service_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&TRACK_SELECT], &[]);
            let run = boot_run(&mut l, &plan, &[&TRACK_SELECT], false);
            assert_ne!(run.verdict(0), Verdict::Accepted, "{}: the module needs a service the card does not hold", dev.name);
        }
        let _ = clean_state();
    }
}

/// `samples.dks` is on the card and the image set has no sample-pool adapter (the DN2 device set): the plan refuses the user of the samples service with a reason, and
/// nothing is loaded.
#[test]
fn a_sample_pool_user_is_refused_when_the_image_has_no_sample_pool_adapter() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware() && d.name == "dn2") {
        let image = Image::read(*dev, "device");
        assert!(!image.modules.iter().any(|r| r == "core/sample-pool"), "the set is expected to leave the adapter out");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&TRACK_SELECT], &SERVICES);
            // the search finds the file and cannot tell what the image lacks: the file fills a slot of the sample-pool adapter, the image has no such slot, so the file is refused at its activation (`E_FORMAT`) and the module then misses its service; the stand-in address of its optional imports plays no part
            assert_eq!(plan.np, 1);
            let run = boot_run(&mut l, &plan, &[&TRACK_SELECT], false);
            assert_eq!(records::module(MAX_MODULES).used, 0, "{}: samples.dks loaded without the sample-pool adapter", dev.name);
            assert!(matches!(run.verdict(0), Verdict::Refused(Reason::NeedMissing(_))), "{}: {:?}", dev.name, run.verdict(0));
        }
        let _ = clean_state();
    }
}

/// A module that calls a function of the project service is found by its needs: the search finds `services/project.dks`, the file loads (ABI word, declare check, exports, the
/// stock bytes of its hook sites) and its hooks are installed on the sites of the port file (five on every device: the paste is found through its caller).
#[test]
fn a_project_user_loads_with_the_project_service_file_and_takes_its_hooks() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        unsafe {
            fits(*dev, "emulator", &[&PROJECT_USER], &["project"]);
        }
        let _ = clean_state();
        let file = service_file(*dev, "project", "core/services/project");
        let t = table_of(&file);
        let keys = Keys::parse(&t).unwrap();
        let pf = port_file(*dev);
        let pt = table_of(&pf);
        let port = Keys::parse(&pt).unwrap();
        let present = (0..keys.count(tag::HOOKS))
            .filter(|i| {
                let w = keys.words(tag::HOOKS, *i).unwrap();
                port.find_site(w.0).is_some_and(|s| s.flags & dkneeds::keys::FLAG_ABSENT == 0)
            })
            .count();
        assert_eq!(keys.count(tag::HOOKS), 5, "{}", dev.name);
        assert_eq!(present, 5, "{}: every hook has a site", dev.name);
        // the two functions the service gives modules
        assert_eq!(keys.count(tag::EXPORTS), 2, "{}", dev.name);
    }
}

/// The project service reads the slot byte of the firmware at its start (`cf.project.value.slot_byte`): the packed file imports it as `dk_fw_<hash>` and the port file of the
/// device lists it with the address the stock setter's `mvs.b` reads. (A name the port lacks would refuse the file, an absent one would turn the read off.)
#[test]
fn the_project_service_binds_the_firmware_slot_byte_from_the_port_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let dev = *dev;
        // the address the committed port data of the firmware holds for the name
        let toml = std::fs::read_to_string(repo().join(std::format!("ports/{}.toml", dev.port()))).unwrap();
        let block = toml.split("name = \"cf.project.value.slot_byte\"").nth(1).expect("the port data has the slot byte");
        let want = block.lines().find_map(|l| l.strip_prefix("addr = 0x")).expect("an address");
        let want = u32::from_str_radix(want.trim(), 16).unwrap();
        let file = service_file(dev, "project", "core/services/project");
        let (off, n) = (be(&file, dkneeds::H_IMPORT_OFF) as usize, be(&file, dkneeds::H_IMPORT_N) as usize);
        let import = std::format!("dk_fw_{:08x}", name_hash("cf.project.value.slot_byte"));
        let imported = (0..n).any(|i| {
            let at = off + i * dkneeds::CF_IMPORT_LEN as usize;
            let name = &file[at..at + dkneeds::CF_IMPORT_LEN as usize];
            name.starts_with(import.as_bytes()) && name[import.len()] == 0
        });
        assert!(imported, "{}: project.dks does not import {import}", dev.name);
        let pf = port_file(dev);
        let pt = table_of(&pf);
        let port = Keys::parse(&pt).unwrap();
        match port.find_name(dkneeds::keys::NAME_FW, name_hash("cf.project.value.slot_byte"), 0) {
            Some(x) if x.flags & dkneeds::keys::FLAG_ABSENT == 0 => assert_eq!(x.addr, want, "{}: the slot byte", dev.name),
            other => panic!("{}: cf.project.value.slot_byte is {other:?} in the port file", dev.name),
        }
        // the file loads against that port file: every name it imports is listed
        unsafe {
            fits(dev, "emulator", &[&PROJECT_USER], &["project"]);
        }
        let _ = clean_state();
    }
}

/// The project service shows its warnings with `dk_ui_open` and `dk_ui_toast`, optional imports of core/ui: the packed file imports both, its needs mark the UI service as
/// optional (a card without core/ui loads it), and the file loads with both imports bound (the stand-in address models the absent stub the device binds).
#[test]
fn the_project_service_imports_the_toast_as_an_optional_need_and_loads_with_it_bound() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let dev = *dev;
        let file = service_file(dev, "project", "core/services/project");
        let (off, n) = (be(&file, dkneeds::H_IMPORT_OFF) as usize, be(&file, dkneeds::H_IMPORT_N) as usize);
        let imports: Vec<String> = (0..n)
            .map(|i| {
                let at = off + i * dkneeds::CF_IMPORT_LEN as usize;
                let name = &file[at..at + dkneeds::CF_IMPORT_LEN as usize];
                String::from_utf8_lossy(&name[..name.iter().position(|b| *b == 0).unwrap()]).to_string()
            })
            .collect();
        for want in ["dk_ui_open", "dk_ui_toast"] {
            assert!(imports.iter().any(|i| i == want), "{}: project.dks does not import {want}", dev.name);
        }
        let t = table_of(&file);
        let keys = Keys::parse(&t).unwrap();
        let ui: Vec<_> = keys.needs().filter(|n| n.service == name_hash("ui")).collect();
        assert_eq!(ui.len(), 1, "{}: one need of the UI service: {ui:?}", dev.name);
        assert!(!ui[0].hard, "{}: the UI service is a hard need of project.dks", dev.name);
        unsafe {
            fits(dev, "emulator", &[&PROJECT_USER], &["project"]);
        }
        let _ = clean_state();
    }
}

/// The same module on a card without `project.dks`: the plan refuses it (its need of the project service is missing), nothing is loaded, and no hook site is written.
#[test]
fn a_project_user_is_refused_without_the_project_service_file_and_every_site_stays_stock() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&PROJECT_USER], &[]);
            let run = boot_run(&mut l, &plan, &[&PROJECT_USER], false);
            assert_ne!(run.verdict(0), Verdict::Accepted, "{}: the module needs a service the card does not hold", dev.name);
            assert_eq!(plan.np, 0, "{}", dev.name);
            assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. } | Call::Word { .. })), "{}: no site is written", dev.name);
        }
        let _ = clean_state();
    }
}

/// A site of the project service whose live bytes are not stock refuses the file with `E_HOOK_STOCK` and writes nothing (the service is not loaded: stock works).
#[test]
fn the_project_service_is_refused_when_a_site_of_its_hooks_is_not_stock() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&PROJECT_USER], &SERVICES);
            let port = (0..plan.np).find(|j| l.found.path(*j).starts_with(b"ports/")).expect("the port file is found");
            let j = (0..plan.np).find(|j| l.found.path(*j) == b"services/project.dks").expect("project.dks is found");
            l.phase = Phase::Boot;
            let ppath = std::format!("ports/{}.dkport\0", dev.port());
            assert_eq!(l.load_provider(ppath.as_ptr(), port), OK, "{}", dev.name);
            let pf = port_file(*dev);
            let pt = table_of(&pf);
            let pk = Keys::parse(&pt).unwrap();
            let file = service_file(*dev, "project", "core/services/project");
            let ft = table_of(&file);
            let fk = Keys::parse(&ft).unwrap();
            // every hook of the file is a plain copy that the port file checks: any one of them, changed, refuses the file
            let sites: Vec<u32> = (0..fk.count(tag::HOOKS))
                .map(|i| fk.words(tag::HOOKS, i).unwrap().0)
                .filter_map(|h| pk.find_site(h))
                .filter(|s| s.flags & dkneeds::keys::FLAG_ABSENT == 0)
                .map(|s| s.addr)
                .collect();
            assert!(sites.len() >= 4, "{}: {sites:?}", dev.name);
            l.p.put_code(sites[0], &[0xff]);
            assert_eq!(l.load_provider(b"services/project.dks\0".as_ptr(), j), E_HOOK_STOCK, "{}", dev.name);
            assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. } | Call::Word { .. })), "no site is written");
            assert_eq!(l.installer.count(), 0);
        }
        let _ = clean_state();
    }
}

/// An import of an image adapter that the host cannot link is accepted when the image set has the adapter, and not when it lacks it.
#[test]
fn an_adapter_import_resolves_only_in_an_image_with_the_adapter() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let file = packed(*dev, TRACK_SELECT.leaf, "dkm", TRACK_SELECT.nx);
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&TRACK_SELECT], &SERVICES);
            let _run = boot_run(&mut l, &plan, &[&TRACK_SELECT], true);
            assert!(imports_resolve(&mut l, &image, &file, "track-select"), "{}: the image has core/ui", dev.name);
            let bare = Image { entries: Vec::new(), modules: Vec::new() };
            let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| imports_resolve(&mut l, &bare, &file, "track-select")));
            assert!(refused.is_err(), "{}: an image without core/ui accepted dk_ui_*", dev.name);
        }
        let _ = clean_state();
    }
}

/// The tables the machines and the fx service read through `CoreApi::info` are in the info table of every patched image of a device with a DSP, each with its magic word and
/// the device it is for (the services refuse a table of another device); an image without the adapters has neither, and the services find no rows then (their host tests:
/// `tests/guard.rs` of the machines service, `without_the_table_of_the_effect_chain_the_service_has_no_rows_and_no_slot` of the fx service).
#[test]
fn the_image_info_table_holds_the_dsp_and_effect_tables_the_services_read() {
    use dkrt_abi::dsp::{FX_INFO_MAGIC, FX_ROW_WORDS, F_DEVICE, F_MAGIC, F_WORDS, INFO_MAGIC, I_DEVICE, I_MAGIC};
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for set in ["emulator", "device"] {
            let image = Image::read(*dev, set);
            let table = |name: &str| image.entries.iter().find(|e| e.1 == name_hash(name) && e.2 == 0).map(|e| e.4.clone());
            let dsp = table("dyndsp_info").unwrap_or_else(|| panic!("{} {set}: no dyndsp_info", dev.name));
            assert_eq!((dsp[I_MAGIC], dsp[I_DEVICE]), (INFO_MAGIC, dev.profile.id), "{} {set}: dyndsp_info", dev.name);
            let fx = table("dynfx_info").unwrap_or_else(|| panic!("{} {set}: no dynfx_info", dev.name));
            assert_eq!((fx[F_MAGIC], fx[F_DEVICE], fx[F_WORDS]), (FX_INFO_MAGIC, dev.profile.id, FX_ROW_WORDS), "{} {set}: dynfx_info", dev.name);
            // an image without the adapters: the platform has no entry of that name
            let bare = Image { entries: Vec::new(), modules: Vec::new() }.platform(*dev, &[]);
            unsafe {
                use dkrt_abi::platform::Platform;
                assert!(Platform::info(&bare, name_hash("dyndsp_info")).is_null() && Platform::info(&bare, name_hash("dynfx_info")).is_null());
                let full = image.platform(*dev, &[]);
                assert!(!Platform::info(&full, name_hash("dyndsp_info")).is_null() && !Platform::info(&full, name_hash("dynfx_info")).is_null());
            }
        }
    }
}

#[test]
fn a_c_service_with_the_abi_word_0_is_accepted_and_its_user_loads() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        unsafe {
            fits(*dev, "emulator", &[&SVC_USER], &["svc-test"]);
        }
        let _ = clean_state();
    }
}

/// All services of a card at once, on both images of the device: the hook sites of the files do not overlap, the rows are all there, and the capacity is the image's.
#[test]
fn every_service_of_the_card_loads_together_on_the_emulator_and_the_device_image() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for set in ["emulator", "device"] {
            let mut modules: Vec<&Fixture> = std::vec![&SINE, &FX_GAIN, &LFO_SHAPE, &SVC_USER, &SHORTCUT_DEMO, &PROJECT_USER];
            let mut want = std::vec!["machines", "fx", "lfo", "svc-test", "shortcuts", "project"];
            // the samples service file needs the sample-pool adapter (`requires`), which the DN2 device image leaves out (the DN2 pool is not proven on hardware)
            if !(dev.name == "dn2" && set == "device") {
                modules.push(&TRACK_SELECT);
                want.push("samples");
            }
            if dev.name == "dn2" {
                modules.push(&DYN_DN2);
            }
            unsafe {
                fits(*dev, set, &modules, &want);
            }
            let _ = clean_state();
        }
    }
}

/// The capacity limits come from the image's `cap_<kind>` entries before any service file loads: the entries are in the image, and a module is judged by them.
#[test]
fn the_image_gives_the_limits_the_plan_judges_by() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        for kind in [CAPK_MACHINE_ROWS, CAPK_PARAM_ROWS, CAPK_PAGE_ROWS, CAPK_FX_SLOTS] {
            assert!(image.has(cap_hash(kind)), "{}: the image has no cap_ entry with a word for capacity kind {kind}", dev.name);
        }
        // a limit under the uses of the module refuses it, naming the resource
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[(CAPK_MACHINE_ROWS, 0)], &[&SINE], &SERVICES);
            let run = boot_run(&mut l, &plan, &[&SINE], false);
            assert_eq!(run.verdict(0), Verdict::Refused(Reason::Fit(CAPK_MACHINE_ROWS)), "{}", dev.name);
            assert_eq!(run.verdict(0).result(), E_PLAN_FIT);
            assert_eq!(run.code(0), E_PLAN_FIT);
        }
        let _ = clean_state();
    }
}

#[test]
fn a_card_without_the_service_file_refuses_its_user_naming_the_service() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&SINE], &SERVICES[..1]); // fx only
            let run = boot_run(&mut l, &plan, &[&SINE], false);
            assert_eq!(run.verdict(0), Verdict::Refused(Reason::NeedMissing(name_hash("machines"))), "{}", dev.name);
            assert_eq!(run.verdict(0).result(), E_MISSING);
        }
        let _ = clean_state();
    }
}

/// One key rule for every kind: the same packed module twice has the same machine key, so the rules of the core refuse the second with the kind and the key that the first
/// has (`E_PLAN_KEY`), on both devices. A different module with other keys is accepted next to the first.
#[test]
fn the_same_machine_key_in_two_packed_modules_is_refused_by_the_boot_plan() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let table = table_of(&packed(*dev, SINE.leaf, "dkm", SINE.nx));
        let key = Keys::parse(&table).unwrap().keys_of(dkneeds::KIND_MACHINE).next().expect("the sine fixture has a machine key");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&SINE, &SINE], &SERVICES[1..2]); // machines only
            // without the DSP section, which the host does not load: the first module goes up (the keys are the same bytes)
            swap_module(&mut l, 0, SINE.leaf, without_dsp(*dev, &SINE));
            swap_module(&mut l, 1, SINE.leaf, without_dsp(*dev, &SINE));
            let _dsp = dsp_env(*dev, &l, &plan);
            let run = boot_run(&mut l, &plan, &[&SINE, &SINE], false);
            assert_eq!(run.verdict(0), Verdict::Accepted, "{}", dev.name);
            assert_eq!(run.verdict(1), Verdict::Refused(Reason::DuplicateKey(dkneeds::KIND_MACHINE, key)), "{}", dev.name);
            assert_eq!(run.verdict(1).result(), dkrt_sdk::E_PLAN_KEY);
            // the first module is intact: up, with its own blocks in the arena and nothing of the second (the blocks equal those of a boot of the first module alone)
            assert!(run.is_up(PackageId(0)), "{}: the first module stays up: {:?}", dev.name, run.refusal(0));
            assert!(!run.is_up(PackageId(1)), "{}", dev.name);
            let blocks = |l: &Boot| l.m.live.iter().map(|b| b.1.len()).collect::<Vec<_>>();
            let both = blocks(&l);
            drop(_dsp);
            let _ = clean_state();
            let (mut alone, plan) = planned(*dev, &image, &[], &[&SINE], &SERVICES[1..2]);
            swap_module(&mut alone, 0, SINE.leaf, without_dsp(*dev, &SINE));
            let _dsp = dsp_env(*dev, &alone, &plan);
            let run = boot_run(&mut alone, &plan, &[&SINE], false);
            assert!(run.is_up(PackageId(0)), "{}", dev.name);
            assert_eq!(both, blocks(&alone), "{}: the arena holds the first module's blocks and nothing of the refused one", dev.name);
        }
        let _ = clean_state();
    }
}

/// The same refusal for an effect: its builder calls name the `fx` service, so a card with the machines and lfo services and no `fx.dks` refuses it.
#[test]
fn a_card_without_the_fx_service_file_refuses_an_effect_naming_the_service() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES[1..]); // machines, lfo and the test service
            let run = boot_run(&mut l, &plan, &[&FX_GAIN], false);
            assert_eq!(run.verdict(0), Verdict::Refused(Reason::NeedMissing(name_hash("fx"))), "{}", dev.name);
            assert_eq!(run.verdict(0).result(), E_MISSING);
        }
        let _ = clean_state();
    }
}

/// The shortcuts seam: a module that calls `dk_shortcut_add` has a hard NEED on the service file, its builder import is an export of `shortcuts.dks`, and the file owns the
/// SHORTCUT kind of the module's records. A card without the file refuses the module naming the service.
#[test]
fn a_module_that_binds_a_shortcut_needs_the_shortcuts_service_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let module = table_of(&packed(*dev, SHORTCUT_DEMO.leaf, "dkm", SHORTCUT_DEMO.nx));
        let needs: Vec<(u32, bool)> = Keys::parse(&module).unwrap().needs().map(|n| (n.service, n.hard)).collect();
        // a hard need on the service of its builder call; the other need is the runtime (the module calls `dk_selected_track`, an import of the runtime that the fixture logs)
        assert!(needs.contains(&(name_hash("shortcuts"), true)), "{}: the module's needs {needs:?}", dev.name);
        assert!(needs.iter().all(|n| n.0 == name_hash("shortcuts") || n.0 == name_hash("runtime")), "{}: the module's needs {needs:?}", dev.name);
        let owner = service_file(*dev, "shortcuts", "core/services/shortcuts");
        let t = table_of(&owner);
        let keys = Keys::parse(&t).unwrap();
        let kinds: Vec<u32> = (0..keys.count(tag::KINDS)).map(|i| keys.words(tag::KINDS, i).unwrap().0).collect();
        assert_eq!(kinds, [dkrt_sdk::records::KIND_SHORTCUT], "{}: the kinds shortcuts.dks declares", dev.name);
        // the one hook it takes: the entry of the key walker (ports/fw/input.toml), listed by the port file of the device and present on it
        let hooks: Vec<u32> = (0..keys.count(tag::HOOKS)).map(|i| keys.words(tag::HOOKS, i).unwrap().0).collect();
        assert_eq!(hooks, [name_hash("cf.input.hook.key_event")], "{}: the hooks shortcuts.dks declares", dev.name);
        let image = Image::read(*dev, "emulator");
        unsafe {
            // with the file on the card the module is accepted and the file is the provider that loads, and its builder import is one of its exports
            let (mut l, plan) = planned(*dev, &image, &[], &[&SHORTCUT_DEMO], &SERVICES[5..6]);
            assert!(found(&l, &plan).contains("services/shortcuts.dks"), "{}", dev.name);
            let run = boot_run(&mut l, &plan, &[&SHORTCUT_DEMO], true);
            assert_eq!(run.verdict(0), Verdict::Accepted, "{}", dev.name);
            // the hook site is one `jmp` at the walker entry of the device's firmware
            let binds = binds_of(*dev, &l, &plan);
            assert_eq!(binds.len(), 1, "{}: the sites shortcuts.dks installs", dev.name);
            assert_eq!(binds[0].0, name_hash("cf.input.hook.key_event"), "{}", dev.name);
            assert!(l.exports.owned(name_hash("dk_shortcut_add")).is_some(), "{}: dk_shortcut_add is no export of shortcuts.dks", dev.name);
            let listed: Vec<u32> = (0..keys.count(tag::EXPORTS)).map(|i| keys.words(tag::EXPORTS, i).unwrap().0).collect();
            assert_eq!(listed, [name_hash("dk_shortcut_add")], "{}: the header table's exports", dev.name);
            assert!(import_names(&packed(*dev, SHORTCUT_DEMO.leaf, "dkm", SHORTCUT_DEMO.nx)).iter().any(|n| n == "dk_shortcut_add"), "{}", dev.name);
        }
        let _ = clean_state();
        unsafe {
            // a card without the file refuses it, naming the service
            let (mut l, plan) = planned(*dev, &image, &[], &[&SHORTCUT_DEMO], &SERVICES[..5]);
            let run = boot_run(&mut l, &plan, &[&SHORTCUT_DEMO], false);
            assert_eq!(run.verdict(0), Verdict::Refused(Reason::NeedMissing(name_hash("shortcuts"))), "{}", dev.name);
            assert_eq!(run.verdict(0).result(), E_MISSING);
        }
        let _ = clean_state();
    }
}

/// The builder imports of a service (`provided = true` in sdk/sdk.toml): a module links them to the exports of the file that owns them.
fn builders_of(service: &str) -> Vec<&'static str> {
    dkrt_sdk::api::PROVIDED.iter().filter(|(s, _)| *s == service).map(|(_, n)| *n).collect()
}

/// The names a file imports.
fn import_names(file: &[u8]) -> Vec<String> {
    let (off, n) = (be(file, dkneeds::H_IMPORT_OFF) as usize, be(file, dkneeds::H_IMPORT_N) as usize);
    (0..n)
        .map(|i| {
            let at = off + i * dkneeds::CF_IMPORT_LEN as usize;
            let name = &file[at..at + dkneeds::CF_IMPORT_LEN as usize];
            String::from_utf8_lossy(&name[..name.iter().position(|c| *c == 0).unwrap()]).to_string()
        })
        .collect()
}

/// The seam of the builder calls: every builder import of a module is an export of the service file that owns it, published when that file loaded (the loader links
/// it from the exports, not from the runtime's table). `test/sine` calls the machine builders, `test/dyn-fx-gain` the effect ones.
#[test]
fn the_builder_calls_of_a_module_resolve_to_the_exports_of_the_service_that_owns_them() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for (module, service) in [(&SINE, "machines"), (&FX_GAIN, "fx")] {
            let builders = builders_of(service);
            let image = Image::read(*dev, "emulator");
            unsafe {
                let (mut l, plan) = planned(*dev, &image, &[], &[module], &SERVICES);
                let _dsp = dsp_env(*dev, &l, &plan);
                let _run = boot_run(&mut l, &plan, &[module], true);
                let file = packed(*dev, module.leaf, "dkm", module.nx);
                let used: Vec<String> = import_names(&file).into_iter().filter(|n| builders.contains(&n.as_str())).collect();
                assert!(used.len() >= 3, "{} {}: the module calls {used:?} of the {service} builders", dev.name, module.leaf);
                for n in &used {
                    assert!(l.exports.owned(name_hash(n)).is_some(), "{} {}: {n} is no export of {service}.dks", dev.name, module.leaf);
                }
                // and the file that owns them lists them in its header table
                let owner = service_file(*dev, service, if service == "fx" { "core/services/fx" } else { "core/services/machines" });
                let t = table_of(&owner);
                let keys = Keys::parse(&t).unwrap();
                let listed: Vec<u32> = (0..keys.count(tag::EXPORTS)).map(|i| keys.words(tag::EXPORTS, i).unwrap().0).collect();
                for n in &used {
                    assert!(listed.contains(&name_hash(n)), "{} {service}.dks: the header table does not export {n}", dev.name);
                }
            }
            let _ = clean_state();
        }
    }
}

/// The SDK exports of a service file are its `provided` imports of sdk/sdk.toml, including the internal calls from `handlers::exports`.
#[test]
fn the_header_exports_of_a_service_file_are_its_provided_imports() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for service in ["machines", "fx", "samples"] {
            let owner = service_file(*dev, service, &format!("core/services/{service}"));
            let table = table_of(&owner);
            let keys = Keys::parse(&table).unwrap();
            let listed: std::collections::BTreeSet<u32> = keys.exports()
                .filter(|entry| entry.b & dkneeds::keys::EXPORT_QUERY == 0)
                .map(|entry| entry.a)
                .collect();
            let want: std::collections::BTreeSet<u32> = builders_of(service).into_iter().map(name_hash).collect();
            assert!(!want.is_empty(), "{service} provides nothing in sdk/sdk.toml");
            assert_eq!(listed, want, "{} {service}.dks: header EXPORTS differ from the provided imports", dev.name);
        }
    }
}

/// The packed query is associated with the live provider, not the import table. The host calls the
/// same typed export natively because it cannot execute the packed ColdFire address.
#[test]
fn the_service_query_is_private_to_its_provider_and_reaches_its_typed_dispatcher() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let file = service_file(*dev, LOCAL_PING.leaf, LOCAL_PING.nx);
        let table = table_of(&file);
        let keys = Keys::parse(&table).unwrap();
        let name = name_hash("local-ping");
        let query = name_hash(dkrt_sdk::RESIDENT_QUERY_ENTRY);
        let export = keys.exports().find(|e| e.a == query).expect("local-ping has a query entry");
        assert_eq!(export.b & dkneeds::keys::EXPORT_FLAGS, dkneeds::keys::EXPORT_FLAGS);
        let version = keys.words(tag::PROVIDES, 0).unwrap().1;
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LOCAL_PING_USER], &[LOCAL_PING]);
            assert_eq!(l.resident_query(name, version), Err(E_MISSING));
            let _run = boot_run(&mut l, &plan, &[&LOCAL_PING_USER], true);
            let j = (0..plan.np).find(|j| l.found.path(*j) == b"services/local-ping.dks").unwrap();
            let base = records::module(MAX_MODULES + j).base;
            assert_eq!(l.resident_query(name, version), Ok(base + (export.b & !dkneeds::keys::EXPORT_FLAGS)));
            assert_eq!(l.resident_query(name, version + 1), Err(E_CONTRACT));
            assert_eq!(l.resident_query(name_hash("machines"), version), Err(E_MISSING));
            assert_eq!(l.exports.owned(query), None);
            // An ordinary legacy export with the same hash cannot change which entry the broker uses.
            l.exports.publish_with_scope(MAX_MODULES + j + 1, query, base, true).unwrap();
            assert_eq!(l.resident_query(name, version), Ok(base + (export.b & !dkneeds::keys::EXPORT_FLAGS)));

            // the entry answers from the owner in the file's cell: the boot's `init` started it, so stop it for the "no provider" answer and start it as `init` does
            let ask = dkrt_fixture_local_ping::dk_service_query_v1;
            let mut reply = [0u8; dkrt_fixture_local_ping::COUNTERS_LEN];
            let out = reply.as_mut_ptr().cast();
            let len = reply.len() as u32;
            dkrt_fixture_local_ping::dk_svc_stop();
            assert_eq!(ask(dkrt_fixture_local_ping::OP_COUNTERS, core::ptr::null(), 0, out, len), E_MISSING, "no owner, no provider");
            dkrt_fixture_local_ping::dk_svc_start().expect("the owner starts");
            assert_eq!(ask(dkrt_fixture_local_ping::OP_COUNTERS, core::ptr::null(), 0, out, len), len as i32, "the reply is the counters");
            assert_eq!(ask(dkrt_fixture_local_ping::OP_COUNTERS + 1, core::ptr::null(), 0, out, len), E_NOT_FOUND);
            assert_eq!(ask(dkrt_fixture_local_ping::OP_COUNTERS, core::ptr::null(), 0, out, len - 1), E_TOO_BIG);
        }
        clean_state();
    }
}

/// A service file built for another ABI is refused by its own load; one whose hook site is not stock changes nothing.
#[test]
fn a_provider_with_another_abi_word_or_a_site_that_is_not_stock_is_refused_before_anything_is_written() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            // the word of the file is not the one of the runtime object
            let (mut l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES);
            let j = (0..plan.np).find(|j| l.found.path(*j) == b"services/fx.dks").expect("fx.dks is found");
            let at = l.m.files.iter().position(|(n, _)| n == b"services/fx.dks").unwrap();
            let word = be(&l.m.files[at].1, dkneeds::H_SVC_ABI);
            assert_ne!(word, 0, "fx.dks has an ABI word");
            put(&mut l.m.files[at].1, dkneeds::H_SVC_ABI, word ^ 1);
            seal(&mut l.m.files[at].1);
            l.phase = Phase::Boot;
            let path = "services/fx.dks\0";
            assert_eq!(l.load_provider(path.as_ptr(), j), E_SVC_ABI, "{}", dev.name);
            assert!(l.m.live.is_empty(), "nothing is allocated for a refused file");
        }
        let _ = clean_state();
        unsafe {
            // a hook site whose live bytes are not the stock bytes of the port file's check value: the port file loads first, then the file that takes the site
            let (mut l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES);
            let port = (0..plan.np).find(|j| l.found.path(*j).starts_with(b"ports/")).expect("the port file is found");
            let j = (0..plan.np).find(|j| l.found.path(*j) == b"services/fx.dks").unwrap();
            l.phase = Phase::Boot;
            let ppath = std::format!("ports/{}.dkport\0", dev.port());
            assert_eq!(l.load_provider(ppath.as_ptr(), port), OK, "{}", dev.name);
            // a raw site of fx (the port copies no raw site, so the port file is not what refuses): its address is in the port file
            let pf = port_file(*dev);
            let pt = table_of(&pf);
            let pk = Keys::parse(&pt).unwrap();
            let fxfile = service_file(*dev, "fx", "core/services/fx");
            let ft = table_of(&fxfile);
            let fk = Keys::parse(&ft).unwrap();
            let addr = (0..fk.count(tag::HOOKS))
                .map(|i| fk.words(tag::HOOKS, i).unwrap().0)
                .filter_map(|h| pk.find_site(h))
                .find(|s| s.kind == dkneeds::keys::SITE_RAW && s.flags & dkneeds::keys::FLAG_ABSENT == 0)
                .expect("fx.dks has a raw hook")
                .addr;
            l.p.put_code(addr, &[0xff]);
            assert_eq!(l.load_provider(b"services/fx.dks\0".as_ptr(), j), E_HOOK_STOCK, "{}", dev.name);
            assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. } | Call::Word { .. })), "no site is written");
            assert_eq!(l.installer.count(), 0);
        }
        let _ = clean_state();
    }
}

/// The identity of the firmware: the image (`patchkit apply`) has the entry `fw_id` and the port file the `FW` item, both the first 8 bytes of the SHA-256 of the stock
/// image, so the port file of the device is found on the card and loads. The same file on an image of another firmware is not found by the search; loaded by its path it is
/// refused as a site that is not stock before anything is allocated or written.
#[test]
fn the_port_file_carries_the_identity_of_its_image_and_a_port_file_for_another_firmware_is_not_taken() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let id = image.entries.iter().find(|e| e.1 == name_hash("fw_id")).unwrap_or_else(|| panic!("{}: the image has no fw_id entry", dev.name));
        assert_eq!((id.2, id.4.len() >= 2), (0, true), "{}: fw_id is a data entry of at least two words", dev.name);
        let words = [id.4[0], id.4[1]];
        let pf = port_file(*dev);
        let pt = table_of(&pf);
        let fw = Keys::parse(&pt).unwrap().fw(0).expect("the port file has its FW item");
        let be32 = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        assert_eq!([be32(&fw[..4]), be32(&fw[4..])], words, "{}: the port file is for the firmware of the image", dev.name);
        unsafe {
            // the search takes it
            let (mut l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES);
            let ppath = std::format!("ports/{}.dkport", dev.port());
            assert!((0..plan.np).any(|j| l.found.path(j) == ppath.as_bytes()), "{}: the port file is not found", dev.name);
            // another firmware: the image's id differs by one bit; the search finds no port file, and a load by path is refused
            let h = name_hash("fw_id");
            l.p.info_entries.borrow_mut().iter_mut().find(|(x, _)| *x == h).unwrap().1[1] ^= 1;
            l.find_providers(1);
            assert!(!(0..l.found.n).any(|j| l.found.path(j).starts_with(b"ports/")), "{}: a port file for another firmware was found", dev.name);
            l.phase = Phase::Boot;
            let allocs = l.m.allocs;
            let p0 = std::format!("{ppath}\0");
            assert_eq!(l.load_provider(p0.as_ptr(), 0), E_HOOK_STOCK, "{}", dev.name);
            assert_eq!(l.m.allocs, allocs, "nothing is allocated");
            assert!(!l.p.calls.borrow().iter().any(|c| matches!(c, Call::Write { .. } | Call::Word { .. } | Call::Mask)), "nothing is written");
            assert_eq!(l.installer.count(), 0);
        }
        let _ = clean_state();
    }
}

/// Every hook a service file takes is listed by the port file of the device (present, or flagged absent), and every firmware import of it names an item of the port file
/// (a name, an exit or a site): the seam between the two built artifacts, before any load. A name the port does not list would refuse the service with `E_IMPORT`.
#[test]
fn every_hook_and_firmware_import_of_a_service_file_is_listed_by_the_port_file() {
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let pf = port_file(*dev);
        let pt = table_of(&pf);
        let port = Keys::parse(&pt).unwrap();
        let mut checked = 0;
        for s in &SERVICES {
            let file = service_file(*dev, s.leaf, s.nx);
            let t = table_of(&file);
            let keys = Keys::parse(&t).unwrap();
            for i in 0..keys.count(tag::HOOKS) {
                let w = keys.words(tag::HOOKS, i).unwrap();
                assert!(port.find_site(w.0).is_some(), "{} {}: hook {:08x} is not in the port file", dev.name, s.leaf, w.0);
                checked += 1;
            }
            for n in import_names(&file) {
                if let Some(r) = super::super::port::parse_import(n.as_bytes()) {
                    let lookup = super::super::port::lookup(&port, 0x1000, 16, r);
                    assert!(!matches!(lookup, super::super::port::Lookup::Unlisted(_)), "{} {}: {n} is not in the port file", dev.name, s.leaf);
                    checked += 1;
                }
            }
            // a service that names the firmware needs the port file
            let names_fw = keys.count(tag::HOOKS) != 0 || import_names(&file).iter().any(|n| super::super::port::parse_import(n.as_bytes()).is_some());
            let needs_port = keys.needs().any(|n| n.service == name_hash("port") && n.hard);
            assert_eq!(names_fw, needs_port, "{} {}: a service that names the firmware has a hard need of `port`, and no other has", dev.name, s.leaf);
        }
        assert!(checked > 0, "{}: no hook or firmware import was compared", dev.name);
    }
}

// ---- consistency of the generated artifacts (no firmware needed) ----------------------------------------------------------------

/// The committed objects of the service files carry the ABI word the package object compares with (`dk_svc_abi` against the immediate of `abi_ok`), and the packed
/// file of each device has it in header word 0x3c. A source change that moves `SVC_ABI` leaves a service object stale: the loader then refuses every service file.
#[test]
fn the_service_objects_and_files_carry_the_abi_word_of_the_package_object() {
    let package = package_object();
    for s in SERVICES.iter().filter(|s| s.nx.starts_with("core/") && s.leaf != "samples") {
        let json = read(&repo().join(format!("services/{}/dks/build/{}.dko.json", s.leaf, s.leaf)), "patchkit build --tree services");
        let abi = object_abi(std::str::from_utf8(&json).unwrap()).to_string();
        assert!(
            package.contains(&std::format!("0c80{abi}")),
            "{}.dko.json carries ABI word {abi}, the package object another: patchkit build services/{}/dks",
            s.leaf,
            s.leaf
        );
        for dev in DEVICES {
            let file = packed(dev, s.leaf, "dks", s.nx);
            assert_eq!(std::format!("{:08x}", be(&file, dkneeds::H_SVC_ABI)), abi, "{} {}: the packed file has another ABI word", dev.name, s.leaf);
        }
    }
}

/// The declarations of the real `init` of a service file (its glue, `dk_svc_declare` of `service!`, run natively over the runtime's own `dk_service_declare_*`) against the
/// header table of the file as the card holds it for the device: the same kinds with their lengths, slots with their flags, hooks, exports and resources. The loader makes the same
/// comparison at the commit (`decl_matches`) and refuses a difference with `E_DECL_KIND`; the other cases here replay the table and cannot see one. The entry addresses are not
/// compared: a host function has no offset in the file (an entry is checked by its presence and its name hash).
#[test]
fn the_init_of_each_service_file_declares_what_its_header_table_carries() {
    fn dt2() -> u32 {
        profiles::DT2.id
    }
    fn dn2() -> u32 {
        profiles::DN2.id
    }
    // (tag, a, b) with the address words left out
    fn plain(tag: u16, a: u32, b: u32) -> (u16, u32, u32) {
        // a patch is a `HOOKS` item of the header table (the entry word names it as a patch)
        if tag == Declare::PATCH {
            (Declare::HOOK, a, 0)
        } else if tag == Declare::HOOK || tag == Declare::EXPORT {
            (tag, a, 0)
        } else {
            (tag, a, b)
        }
    }
    fn name(tag: u16) -> &'static str {
        match tag {
            t if t == Declare::KIND => "KINDS",
            t if t == Declare::SLOT => "SLOTS",
            t if t == Declare::HOOK => "HOOKS",
            t if t == Declare::EXPORT => "EXPORTS",
            _ => "RESOURCES",
        }
    }
    let _g = clean();
    // the entries a service declares are host functions here: the map gives them device addresses
    let map = crate::fake_platform::address_map::AddressMap::new();
    let _space = map.install();
    let services: Vec<(&Fixture, crate::package::handlers::dks_decls::DeclareFn)> = crate::package::handlers::dks_decls::declarations()
        .iter()
        .map(|(leaf, declare)| (SERVICES.iter().find(|s| s.leaf == *leaf).unwrap(), *declare))
        .collect();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for (s, declare) in &services {
            // the packed file carries the hooks of both devices (with a device mask in the entry) and the slot fills of both (with a device mask in the flags): the loader drops the masks, the hooks the port file lists as absent and the slots of the other device
            let file = service_file(*dev, s.leaf, s.nx);
            let header = crate::package::collect::KeysCopy::from_bytes(&table_of(&file)).unwrap().decl;
            let pf = port_file(*dev);
            let pt = table_of(&pf);
            let port = Keys::parse(&pt).unwrap();
            let mut want: Vec<(u16, u32, u32)> = (0..header.n)
                .filter(|i| {
                    let d = header.items[*i];
                    // a slot fill of another device's image is skipped by the loader
                    !(d.tag == Declare::HOOK && port.find_site(d.a).is_some_and(|x| x.flags & dkneeds::keys::FLAG_ABSENT != 0))
                        && !(d.tag == Declare::HOOK && !dkneeds::keys::hook_for_device(d.b, dev.profile.id))
                        && !(d.tag == Declare::SLOT && !dkneeds::keys::slot_for_device(d.b, dev.profile.id))
                })
                .map(|i| plain(header.items[i].tag, header.items[i].a, header.items[i].b & !dkneeds::keys::SLOT_DEVICES_MASK))
                .collect();
            let mut got: Vec<(u16, u32, u32)> = Vec::new();
            unsafe {
                let mut col = std::boxed::Box::new(crate::package::collect::Collect::new());
                crate::package::collect::begin(&mut *col);
                let ctx = crate::package::entry::bind(0) as *mut u8;
                let r = declare(ctx, if dev.name == "dt2" { dt2 } else { dn2 });
                crate::package::collect::end();
                crate::package::entry::unbind(0);
                assert_eq!(r, OK, "{} {}: init refused its own declarations", dev.name, s.leaf);
                for i in 0..col.decl.n {
                    let d = col.decl.items[i];
                    // the row and the provider's instance are the core's own, not entries of the header table
                    if d.tag != Declare::PROVIDER_ROW && d.tag != Declare::PROVIDER_INSTANCE {
                        got.push(plain(d.tag, d.a, d.b));
                    }
                }
            }
            // a service may declare a hook plain that the table lists for another device only: the loader drops that declaration at the commit (`declared_offsets`), so it is not
            // compared. Per name, as many declarations go as the table has items of other devices for it, never more than the file declared above the table's own.
            for i in 0..header.n {
                let h = header.items[i];
                if h.tag == Declare::HOOK && !dkneeds::keys::hook_for_device(h.b, dev.profile.id) {
                    let (g, w) = (got.iter().filter(|x| x.0 == Declare::HOOK && x.1 == h.a).count(), want.iter().filter(|x| x.0 == Declare::HOOK && x.1 == h.a).count());
                    if g > w {
                        let at = got.iter().position(|x| x.0 == Declare::HOOK && x.1 == h.a).unwrap();
                        got.remove(at);
                    }
                }
            }
            got.sort();
            want.sort();
            let show = |v: &[(u16, u32, u32)], other: &[(u16, u32, u32)]| {
                v.iter().filter(|x| !other.contains(x)).map(|x| std::format!("{} {:08x}", name(x.0), x.1)).collect::<Vec<_>>()
            };
            assert!(
                got == want,
                "{} {}: init declares what the header table does not carry {:?}; the table carries what init does not declare {:?}",
                dev.name,
                s.leaf,
                show(&got, &want),
                show(&want, &got)
            );
        }
    }
}

/// Between the cases the loader tables are reset (the guard of the test holds the lock for all of them).
pub(super) fn clean_state() {
    unsafe {
        for rec in MAX_MODULES..MAX_MODULES + PROVIDER_MAX {
            crate::package::provider_rows::remove(rec);
        }
        *st() = core::mem::zeroed();
        records::reset();
        *dsp::ds() = core::mem::zeroed();
        dsp::init_once();
        CALLED_NEED = [0; NEEDS_LEN];
    }
}

/// The runtime's boot over real files (`Rt::boot_with`): the accepted module loads and is reported to the boot record, the refused one is reported with the reason and detail words of
/// the rules of the core and its file is never staged.
#[test]
fn the_boot_loads_accepted_modules_and_reports_a_refused_one_without_loading_it() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        if be(&packed(*dev, SVC_USER.leaf, "dkm", SVC_USER.nx), dkneeds::H_DSP_SIZE) != 0 {
            eprintln!("SKIP {}: {} has a DSP section, which the host cannot load", dev.name, SVC_USER.leaf);
            continue;
        }
        unsafe {
            // a machine row limit of 0 refuses `sine` (the rules of the core name the resource); `svc-test-user` needs no machine row
            let (mut l, _plan) = planned(*dev, &image, &[(CAPK_MACHINE_ROWS, 0)], &[&SVC_USER, &SINE], &SERVICES);
            let rec = boot_record(&[SVC_USER.leaf, SINE.leaf]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));

            let b = crate::boot::info();
            assert_eq!(b.results[..2], [OK, E_PLAN_FIT], "{}", dev.name);
            assert_eq!(b.loaded, 1, "{}", dev.name);
            assert_eq!((b.states[0], b.states[1]), (STATE_ACTIVE, STATE_REFUSED), "{}", dev.name);
            assert_eq!(b.detail[1], [CAPK_MACHINE_ROWS as u32, 0], "{}", dev.name);
            // the services were told which modules stay: the refused one takes no number
            assert_eq!((l.accepted[0], l.accepted[1]), (true, false), "{}", dev.name);
            // the accepted module is live in the first record; the refused file was never read from the store
            assert_eq!((st().modules[0].used, st().modules[1].used, st().loads), (1, 0, 1), "{}", dev.name);
            assert_eq!(l.m.staged_path, b"modules/svc-test-user.dkm".to_vec(), "{}: the last file staged", dev.name);
            // each result names its file's leaf
            let leaf = |k: usize| core::slice::from_raw_parts(crate::boot::boot_name(k), dkrt_abi::state::NAME_LEN).to_vec();
            assert_eq!(&leaf(0)[..17], b"svc-test-user.dkm");
            assert_eq!(&leaf(1)[..8], b"sine.dkm");
            // the boot leaves the loader closed
            assert!(l.phase == Phase::Closed);
            assert_eq!(l.load(b"modules/svc-test-user.dkm\0".as_ptr()), E_BOOT_ONLY);
        }
        let _ = clean_state();
    }
}

/// The MODULES reply of a real boot reports each module with the version of its file's header and the runtime version its file needs (what the module-load device test read back
/// from the daemon): the loader keeps both from the file, and the command writes them in the record of the module.
#[test]
fn the_modules_reply_carries_the_version_and_the_needed_runtime_of_each_loaded_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let selection = [&LFO_SHAPE, &LOCAL_PING_USER];
            let services = [Fixture { leaf: SERVICES[2].leaf, nx: SERVICES[2].nx }, Fixture { leaf: LOCAL_PING.leaf, nx: LOCAL_PING.nx }];
            let (mut l, plan) = planned(*dev, &image, &[], &selection, &services);
            let _dsp = dsp_env(*dev, &l, &plan);
            let rec = boot_record(&[LFO_SHAPE.leaf, LOCAL_PING_USER.leaf]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            assert_eq!(crate::boot::info().loaded, 2, "{}: both modules load", dev.name);

            crate::package::module_cmds::cmd_modules(1, [0u8].as_ptr(), 1);
            let rb = crate::package::resident::transport::rbody().add(1);
            assert_eq!(*rb.add(16), 2, "{}: the total of loaded modules", dev.name);
            assert_eq!(*rb.add(17), 2, "{}: both records are in the reply", dev.name);
            let mut at = 18usize;
            let mut versions = Vec::new();
            for m in selection {
                let file = packed(*dev, m.leaf, "dkm", m.nx);
                let word = |off: usize| u32::from_be_bytes(core::slice::from_raw_parts(rb.add(at + off), 4).try_into().unwrap());
                let name = core::slice::from_raw_parts(rb.add(at + 1), 32).iter().take_while(|b| **b != 0).map(|b| *b as char).collect::<String>();
                assert_eq!(name, m.nx, "{}: the module's name", dev.name);
                assert_eq!(word(33), be(&file, 0x34), "{}: {}: the version of the header", dev.name, m.leaf);
                assert_eq!(word(37), be(&file, 0x30) & 0x7fff_ffff, "{}: {}: the runtime version the file needs", dev.name, m.leaf);
                versions.push(word(33));
                // the fixed part ends at dsp_len (61 bytes); then `u8 n_sections` and `u32 hash, u8 n, n words` for each (the lfo shapes and the local-ping user have no machine)
                let mut p = at + 62;
                for _ in 0..*rb.add(at + 61) {
                    p += 5 + 4 * *rb.add(p + 4) as usize;
                }
                at = p;
            }
            // every packed file needs at least the runtime's `[runtime] table` floor, so two files need the same version unless one links a newer import; the two fixtures
            // differ in their own version, so a swapped or constant version field shows
            assert_ne!(versions[0], versions[1], "{}: the two fixtures have different versions, so a swapped or constant field shows", dev.name);
        }
        let _ = clean_state();
    }
}

// ---- RC2b: what the device tests showed once and the host can show ---------------------------------------------------------

/// Where entry `tag` of the header table of `f` has its payload (an offset in the file), and its item count.
fn entry_of(f: &[u8], tag: u16) -> Option<(usize, usize)> {
    let table = be(f, dkneeds::H_REG_OFF) as usize;
    let u16_at = |at: usize| u16::from_be_bytes([f[at], f[at + 1]]) as usize;
    let mut at = table + 8;
    for _ in 0..u16_at(table + 6) {
        let len = be(f, at as u32 + 4) as usize;
        if u16_at(at) == tag as usize {
            return Some((at + 8, u16_at(at + 2)));
        }
        at += 8 + len;
    }
    None
}

/// Replace the file `leaf` of the card by `bytes` (sealed), in the store and in the capture of selection index `k`.
pub(super) unsafe fn swap_module(l: &mut Boot, k: usize, leaf: &str, mut bytes: Vec<u8>) {
    seal(&mut bytes);
    let at = l.m.files.iter().position(|(n, _)| *n == std::format!("modules/{leaf}.dkm").as_bytes()).unwrap();
    l.m.files[at].1 = bytes.clone();
    l.capture(k, &bytes);
}

/// The `since` of every import that has a service (`[[import]]` of `sdk/sdk.toml`), packed major << 16 | minor << 8 | patch, and the runtime version that introduced the
/// header table (`[runtime] table`, the least a packed module needs), and the two versions that a service-local API and a resident query raise it to.
fn sdk_versions() -> (std::collections::BTreeMap<String, u32>, u32, [u32; 2]) {
    let text = String::from_utf8(read(&repo().join("sdk/sdk.toml"), "git checkout sdk/sdk.toml")).unwrap();
    let sdk: toml::Table = text.parse().unwrap();
    let pack = |s: &str| {
        let v: Vec<u32> = s.split('.').map(|n| n.parse().unwrap()).collect();
        v[0] << 16 | v[1] << 8 | v[2]
    };
    let since = sdk["import"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|i| i.get("service").is_some())
        .map(|i| (i["name"].as_str().unwrap().to_string(), pack(i["since"].as_str().unwrap())))
        .collect();
    let rt = |k: &str| pack(sdk["runtime"][k].as_str().unwrap());
    (since, rt("table"), [rt("service_registration"), rt("resident_query")])
}

/// What is wrong with the runtime-need word (header word 0x30, without the trace flag) of the packed module `f`, if anything: it is the newest `since` of the service
/// imports it links (at least the version of the header table; a module with a service-local API has the registration version, which is above both), and it names the
/// runtime's own major and minor with a patch the runtime has.
fn runtime_word_problem(f: &[u8], since: &std::collections::BTreeMap<String, u32>, table: u32, floors: [u32; 2]) -> Option<String> {
    let word = be(f, dkneeds::H_RUNTIME) & !crate::state::FLAG_TRACE;
    let want = import_names(f).iter().filter_map(|n| since.get(n)).copied().max().unwrap_or(0).max(table);
    let v = dkrt_sdk::RUNTIME_VERSION;
    if word != want && !(word > want && floors.contains(&word)) {
        Some(std::format!("the word is {word:#x}, the newest since of its imports is {want:#x}"))
    } else if word >> 8 != v >> 8 || word & 255 > v & 255 {
        Some(std::format!("the word {word:#x} is not a need the runtime {v:#x} meets"))
    } else {
        None
    }
}

/// The seam of `patchkit pack` and the runtime's version: every packed module of the fixtures carries the newest `since` of its imports, and it is a version of the runtime's
/// major and minor, so a drift between patchkit's `sdk/sdk.toml` and the runtime's `RUNTIME_VERSION` fails here and not as E_CONTRACT on a device.
#[test]
fn a_packed_module_needs_the_newest_since_of_its_imports_and_the_runtime_major_and_minor() {
    let (since, table, floors) = sdk_versions();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for m in [&SINE, &FX_GAIN, &LFO_SHAPE, &SVC_USER, &LOCAL_PING_USER, &SHORTCUT_DEMO, &PROJECT_USER, &TRACK_SELECT] {
            let file = packed(*dev, m.leaf, "dkm", m.nx);
            assert_eq!(runtime_word_problem(&file, &since, table, floors), None, "{} {}", dev.name, m.leaf);
        }
        // the check fails when the seam drifts: a newer minor, a module that needs more than its imports, one that needs less than the newest of them
        let mut file = packed(*dev, SINE.leaf, "dkm", SINE.nx);
        let word = be(&file, dkneeds::H_RUNTIME);
        assert!(dkrt_sdk::RUNTIME_VERSION & 255 != 255);
        for drifted in [word + 0x100, word + 0x7f, word - 0x100] {
            put(&mut file, dkneeds::H_RUNTIME, drifted);
            assert!(runtime_word_problem(&file, &since, table, floors).is_some(), "{} {drifted:#x}", dev.name);
        }
        put(&mut file, dkneeds::H_RUNTIME, word);
        let mut older = since.clone();
        for n in import_names(&file) {
            if let Some(v) = older.get_mut(&n) {
                *v = table + 1;
            }
        }
        assert!(runtime_word_problem(&file, &older, table, floors).is_some(), "{}: a newer `since` than the word", dev.name);
    }
}

/// The patched copy of `leaf`, with its DSP section dropped (the host does not load one, so the load reaches `init` and the commit).
pub(super) fn without_dsp(dev: Dev, m: &Fixture) -> Vec<u8> {
    let mut f = packed(dev, m.leaf, "dkm", m.nx);
    put(&mut f, dkneeds::H_DSP_SIZE, 0);
    f
}

/// What a refused boot leaves: no block of the card is live (the module's own and the services started for it are given back), no module record is used, and the table of the
/// services holds none of the files this boot loaded.
unsafe fn nothing_left(l: &Boot, what: &str) {
    assert!(l.m.live.is_empty(), "{what}: arena blocks are live");
    assert_eq!((st().modules[0].used, st().loads), (0, 0), "{what}: a module record is used");
    assert!(!["fx", "machines", "lfo"].iter().any(|n| crate::package::provider_rows::has(n)), "{what}: service rows are left");
}

/// E_DECL_ID through the real path: the packed module goes through the loader and the real effects service (its builder and its commit check); `init` declares the effect
/// of index 1 while the module's header table has one effect id (index 0). The load is refused with E_DECL_ID, and the arena, the module records and the service rows hold
/// nothing of the refused boot. (The host runs no module code: the builder call is listed in `Card::init_calls`; the accepted commit needs the DSP rows, a device test.)
#[test]
fn an_effect_index_that_the_keys_table_lacks_is_refused_with_e_decl_id_and_leaves_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES);
            assert_eq!(dkneeds::keys::Keys::parse(&table_of(&packed(*dev, FX_GAIN.leaf, "dkm", FX_GAIN.nx))).unwrap().keys_of(dkneeds::KIND_EFFECT).count(), 1, "the fixture has one effect id");
            let mut f = without_dsp(*dev, &FX_GAIN);
            // the module has a `fini` (the host runs no code: the entry is the `init` offset)
            let init = be(&f, dkneeds::H_INIT);
            put(&mut f, dkneeds::H_FINI, init);
            swap_module(&mut l, 0, FX_GAIN.leaf, f);
            l.m.init_calls = std::vec![Builder::Fx(1)];
            l.m.real_rows = true;
            let _dsp = dsp_env(*dev, &l, &plan);
            let run = boot_run(&mut l, &plan, &[&FX_GAIN], false);
            assert_eq!(run.code(0), E_DECL_ID, "{}: {:?}", dev.name, run.refusal(0));
            nothing_left(&l, dev.name);
            // `init` returned OK and the commit refused: a content module's `fini` does not run (only a provider file's does)
            assert!(!l.m.hooks.iter().any(|(fini, rec)| *fini && *rec < MAX_MODULES), "{}: no fini of a content module", dev.name);
        }
        let _ = clean_state();
    }
}

/// The same for a machine: the machines service refuses a machine index that the keys table lacks (the header table of `sine` has one machine key, index 0).
#[test]
fn a_machine_index_that_the_keys_table_lacks_is_refused_with_e_decl_id_and_leaves_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&SINE], &SERVICES);
            assert_eq!(Keys::parse(&table_of(&packed(*dev, SINE.leaf, "dkm", SINE.nx))).unwrap().keys_of(dkneeds::KIND_MACHINE).count(), 1, "the fixture has one machine key");
            swap_module(&mut l, 0, SINE.leaf, without_dsp(*dev, &SINE));
            // the machines service reads the image's registry table through the core
            let rows_words = image.words("rows_info");
            let _reg = crate::package::cleanup_tests::RowsInfoGuard::set(rows_words.as_ptr());
            l.m.init_calls = std::vec![Builder::Machine(1)];
            l.m.real_rows = true;
            let _dsp = dsp_env(*dev, &l, &plan);
            let run = boot_run(&mut l, &plan, &[&SINE], false);
            assert_eq!(run.code(0), E_DECL_ID, "{}: {:?}", dev.name, run.refusal(0));
            nothing_left(&l, dev.name);
        }
        let _ = clean_state();
    }
}

/// More rows declared than the file's `[uses]` count is refused with E_DECL_USES through the real path: the packed `sine` has its machine-row count set to 0 in the USES entry of its
/// header table while `init` declares its machine, so the commit of the machines service refuses it. The boot is built so that the machine-row count is the only count that can
/// trip the check: `add_machine` declares no params and the profile words after the image's listed head are zero (no page rows). The refusal is
/// the one of the commit (`Failed` entering `Started`). The arena, the module records and the service rows hold nothing of the refused boot. (The check itself is
/// `services/machines/tests/build.rs`; the device test of the commit keeps the accepted commit in the DSP voice row.)
#[test]
fn a_machine_declared_beyond_its_uses_is_refused_with_e_decl_uses_and_leaves_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&SINE], &SERVICES);
            let mut f = without_dsp(*dev, &SINE);
            let (at, n) = entry_of(&f, 2).expect("the fixture has uses");
            let item = (0..n).map(|k| at + 8 * k).find(|a| be(&f, *a as u32) == dkrt_sdk::records::RES_MACHINE_ROWS).expect("the fixture uses machine rows");
            assert!(be(&f, item as u32 + 4) >= 1, "{}: the fixture declares a machine row", dev.name);
            put(&mut f, item as u32 + 4, 0);
            swap_module(&mut l, 0, SINE.leaf, f);
            // the image lists the first words of the table; the machines service reads the profile words after them, which are zero here instead of what follows the vector in memory
            let mut rows_words = image.words("rows_info");
            rows_words.resize(128, 0);
            let _reg = crate::package::cleanup_tests::RowsInfoGuard::set(rows_words.as_ptr());
            l.m.init_calls = std::vec![Builder::Machine(0)];
            l.m.real_rows = true;
            let _dsp = dsp_env(*dev, &l, &plan);
            let run = boot_run(&mut l, &plan, &[&SINE], false);
            assert!(
                matches!(run.refusal(0), Some(LifecycleError::Failed { entering: State::Started, status: E_DECL_USES, .. })),
                "{}: {:?}",
                dev.name,
                run.refusal(0)
            );
            nothing_left(&l, dev.name);
        }
        let _ = clean_state();
    }
}

/// E_INIT through the real loader: the packed `lfo-shape` with its `init` replaced by `moveq #-9,%d0; rts` (the one instruction pair the host decodes) is refused with E_INIT
/// after the plan accepted it, and leaves nothing behind: no arena block, no module record, no row of the service started for it.
#[test]
fn an_init_that_returns_non_zero_is_refused_with_e_init_and_leaves_nothing() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let mut f = packed(*dev, LFO_SHAPE.leaf, "dkm", LFO_SHAPE.nx);
            assert_eq!(be(&f, dkneeds::H_DSP_SIZE), 0, "lfo-shape has no DSP section");
            let at = (be(&f, dkneeds::H_IMAGE_OFF) + be(&f, dkneeds::H_INIT)) as usize;
            f[at..at + 4].copy_from_slice(&[0x70, 0xf7, 0x4e, 0x75]);
            swap_module(&mut l, 0, LFO_SHAPE.leaf, f);
            let run = boot_run(&mut l, &plan, &[&LFO_SHAPE], false);
            assert_eq!(run.verdict(0), Verdict::Accepted, "{}: the plan accepts the module", dev.name);
            assert_eq!(run.code(0), E_INIT, "{}: {:?}", dev.name, run.refusal(0));
            nothing_left(&l, dev.name);
        }
        let _ = clean_state();
    }
}

/// The result, state and detail words of `DKRT_BOOT` for a boot of the packed `lfo-shape` with `edit` applied to its file, over the services of the card.
unsafe fn boot_record_of_edited_lfo(dev: Dev, image: &Image, edit: impl Fn(&mut Vec<u8>)) -> (i32, u8, [u32; 2]) {
    let (mut l, _plan) = planned(dev, image, &[], &[&LFO_SHAPE], &SERVICES);
    let mut f = packed(dev, LFO_SHAPE.leaf, "dkm", LFO_SHAPE.nx);
    edit(&mut f);
    swap_module(&mut l, 0, LFO_SHAPE.leaf, f);
    let rec = boot_record(&[LFO_SHAPE.leaf]);
    let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
    rt.boot_with(&rec, &capacity(&l.p));
    let b = crate::boot::info();
    (b.results[0], b.states[0], b.detail[0])
}

/// The detail words of `DKRT_BOOT` after a real boot (what the module states and the LOADS command show): E_PLAN_KIND names the kind that has no owner, E_PLAN_FIT the
/// resource that is full (the arena, for record bytes above what the device has).
#[test]
fn a_boot_leaves_the_kind_and_the_full_resource_in_the_detail_words_of_a_refusal() {
    let _g = clean();
    const UNOWNED_KIND: u32 = 90;
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            // the first key group of the KEYS entry (tag 3) is of the lfo kind: it becomes a kind nobody owns
            let b = boot_record_of_edited_lfo(*dev, &image, |f| {
                let (at, _) = entry_of(f, 3).expect("the fixture has keys");
                assert_eq!(be(f, at as u32), dkneeds::KIND_LFO_SHAPE);
                put(f, at as u32, UNOWNED_KIND);
            });
            assert_eq!(b, (E_PLAN_KIND, STATE_REFUSED, [UNOWNED_KIND, 0]), "{}", dev.name);
        }
        let _ = clean_state();
        unsafe {
            // the USES entry (tag 2) asks for more record bytes than the arena has
            let b = boot_record_of_edited_lfo(*dev, &image, |f| {
                let (at, n) = entry_of(f, 2).expect("the fixture has uses");
                let item = (0..n).map(|k| at + 8 * k).find(|a| be(f, *a as u32) == dkrt_sdk::records::RES_RECORD_BYTES).expect("the fixture uses record bytes");
                put(f, item as u32 + 4, 0x0100_0000);
            });
            assert_eq!(b, (E_PLAN_FIT, STATE_REFUSED, [CAPK_ARENA as u32, 0]), "{}", dev.name);
        }
        let _ = clean_state();
    }
}

/// What `DKRT_BOOT` holds for each way a module of the selection ends (what the module states and the LOADS command show of it): a file whose hash is wrong is FAILED with `E_SHA` and the module before it is ACTIVE
/// and the only one loaded. (A missing file and a second owner of a key: `manager_assembly_tests.rs`. Moved down from `refusal.device.test.ts`: only the agreement of the views is the firmware's to show.)
#[test]
fn a_boot_records_a_file_with_a_wrong_hash_as_failed_with_e_sha() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            // a byte of the image changed after the file was sealed: the loader's hash check fails, the good module before it loads
            let (mut l, _plan) = planned(*dev, &image, &[], &[&LFO_SHAPE, &LFO_SHAPE_B], &SERVICES);
            let at = l.m.files.iter().position(|(n, _)| *n == b"modules/lfo-shape-b.dkm".to_vec()).unwrap();
            let image_off = be(&l.m.files[at].1, dkneeds::H_IMAGE_OFF) as usize;
            l.m.files[at].1[image_off + 2] ^= 1;
            let rec = boot_record(&[LFO_SHAPE.leaf, LFO_SHAPE_B.leaf]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));
            let b = crate::boot::info();
            assert_eq!(b.results[..2], [OK, E_SHA], "{}", dev.name);
            assert_eq!((b.states[0], b.states[1]), (STATE_ACTIVE, STATE_FAILED), "{}", dev.name);
            assert_eq!(b.loaded, 1, "{}: only the good module", dev.name);
        }
        let _ = clean_state();
    }
}

/// A service that is refused because a need failed names that need on its provider line: the test service needs the port file for its hook site; the port file's check value
/// is changed, so the port file is refused (`E_HOOK_STOCK`, its own hash) and the service, never placed, is `E_MISSING` with the hash of the port file (the root cause), not
/// its own hash.
#[test]
fn a_service_refused_for_a_need_that_failed_names_the_hash_of_that_need() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, _plan) = planned(*dev, &image, &[], &[&SVC_USER], &SERVICES);
            // the hook of the test service and its item in the port file's SITES entry: one byte of the check value
            let hook = Keys::parse(&table_of(&service_file(*dev, SVC_TEST.leaf, SVC_TEST.nx))).unwrap().words(tag::HOOKS, 0).unwrap().0;
            let port = l.m.files.iter().position(|(n, _)| n.starts_with(b"ports/")).unwrap();
            let (sites, n) = entry_of(&l.m.files[port].1, dkneeds::keys::tag::SITES as u16).expect("the port file has sites");
            let item = (0..n).map(|k| sites + dkneeds::keys::SITE_BYTES * k).find(|a| be(&l.m.files[port].1, *a as u32) == hook).expect("the port file lists the hook");
            l.m.files[port].1[item + 12] ^= 1;
            seal(&mut l.m.files[port].1);

            *core::ptr::addr_of_mut!(crate::dlog::DKRT_LOG) = core::mem::zeroed();
            let rec = boot_record(&[SVC_USER.leaf]);
            let mut rt = Box::new(crate::package::lifecycle::Rt::new(&mut l as *mut _));
            rt.boot_with(&rec, &capacity(&l.p));

            // the provider lines of the log: (first characters of the file name, result, service hash, base), in order
            let mut lines: Vec<(u32, u32, i32, u32, u32)> = (*core::ptr::addr_of!(crate::dlog::DKRT_LOG))
                .iter()
                .filter(|e| e.seq != 0 && e.kind == logk::MOD_LOAD && e.a[0] == PROVIDER_IDX)
                .map(|e| (e.seq, e.a[2], e.a[1] as i32, e.a[3], e.a[4]))
                .collect();
            lines.sort();
            let name = |s: &[u8; 4]| u32::from_be_bytes(*s);
            let of = |chars: &[u8; 4]| lines.iter().filter(|x| x.1 == name(chars)).map(|x| (x.2, x.3, x.4)).collect::<Vec<_>>();
            let port_hash = name_hash("port");
            assert_eq!(of(b"svc-"), [(E_MISSING, port_hash, 0)], "{}: the service names the port file, not itself ({:#x}); lines {lines:x?}", dev.name, name_hash("svc-test"));
            assert_eq!(of(&port_name(dev))[0].0, E_HOOK_STOCK, "{}: the port file is refused for its check value", dev.name);
            assert_ne!(port_hash, name_hash("svc-test"));
            assert_eq!(*crate::boot::info().results.first().unwrap(), E_MISSING, "{}: the user is refused too", dev.name);
        }
        let _ = clean_state();
    }
}

/// The first four characters of the port file's name, as a provider line carries them.
fn port_name(dev: &Dev) -> [u8; 4] {
    dev.port().as_bytes()[..4].try_into().unwrap()
}
