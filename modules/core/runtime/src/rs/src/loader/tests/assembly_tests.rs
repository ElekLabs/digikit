//! Assembly tests (docs/plans/test-architecture.md T2 step 4): the seams between separately built artifacts, run on the host with the real packed outputs.
//!
//! Nothing here is a fake of an artifact. The files are the ones the build leaves in `dist/`: the packed fixtures (`dist/modules/<device>/*.dkm`), the service files
//! (`core/services/{fx,machines,lfo}`, `test/svc-test`) as the card holds them (the packed file of `dist/modules/<device>/`, and the port file of `dist/ports/` beside them), the committed
//! runtime object (`modules/core/runtime/build/runtime.dko.json`, the immediate of its ABI check) and the info table of the patched image
//! (`dist/bytes/images/<device>/set-<set>.info`, written by `patchkit:device-images` from the plan of `patchkit apply`). They go through the code of the boot:
//! the search for providers on the card, the plan (needs, owners of kinds, capacity limits from the `cap_<kind>` entries), the role checks of a provider file (ABI word, stock
//! bytes of every hook site), the install of its hooks and slots over `FakePlatform`, and the load of a module without a DSP section.
//!
//! What the host cannot show stays out: no ColdFire code runs (in the loader cases the `init` of a service replays what its header table declares; one case runs the glue of each Rust service file natively and compares), and the DSP part of a module is
//! not loaded (its DSP loader needs the DSP). One case per seam shows that it fits and one the refusal that is typical for it. The edge cases of each part are unit
//! tests of that part; an assertion that needs a hand-built table or a changed rule belongs there, not here.
//!
//! The tests need the outputs of the Nx target `core/runtime:test-assembly` (its `dependsOn`). A device without its firmware image is skipped, as its device tests are.
use super::*;
use crate::fake_platform::{Call, FakePlatform};
use crate::test_lock::LOCK;
use dkneeds::keys::{name_hash, tag, Keys};
use dkrt_abi::plan::{Reason, Verdict};
use dkrt_abi::profiles;
use dkrt_abi::service::{Declare, Service, ServiceRow};
use dkrt_sdk::proto::*;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::string::String;
use std::vec::Vec;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../..")
}

#[derive(Clone, Copy)]
struct Dev {
    name: &'static str,
    profile: dkrt_abi::platform::DeviceProfile,
    /// The stock firmware the images are made from (the acceptance cells); its sites' stock bytes come from `patchkit port stock`.
    firmware: &'static str,
    /// The device file whose `[patch_sets]` name the roots of each image set.
    toml: &'static str,
}

const DT2: Dev = Dev { name: "dt2", profile: profiles::DT2, firmware: "firmware/Digitakt_II_OS1.17.syx", toml: "devices/digitakt-ii.toml" };
const DN2: Dev = Dev { name: "dn2", profile: profiles::DN2, firmware: "firmware/Digitone_II_OS1.12.syx", toml: "devices/digitone-ii.toml" };
const DEVICES: [Dev; 2] = [DT2, DN2];

impl Dev {
    /// The name of the port file of the firmware: `<device>-<version>`, the version from the firmware file's name (`..._OS1.17.syx`).
    fn port(&self) -> String {
        let stem = self.firmware.rsplit('/').next().unwrap().trim_end_matches(".syx");
        std::format!("{}-{}", self.name, stem.rsplit("OS").next().unwrap())
    }

    /// False (with a note) when the firmware is not here: its images are not made, and nothing of this device can be assembled.
    fn has_firmware(&self) -> bool {
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
fn packed(dev: Dev, leaf: &str, ext: &str, nx: &str) -> Vec<u8> {
    read(&repo().join(format!("dist/modules/{}/{leaf}.{ext}", dev.name)), &format!("pnpm nx:pilot run {nx}:pack"))
}

/// A service file as the card holds it: the packed file, which names the firmware only by imports and holds no firmware bytes.
fn service_file(dev: Dev, leaf: &str, nx: &str) -> Vec<u8> {
    packed(dev, leaf, "dks", nx)
}

/// The stock bytes of every hook site a service takes, from the local firmware (`patchkit port stock`, which reads the image and the committed port data; the bytes go
/// into a fake firmware here and are written nowhere): hook name hash to (site address, displaced bytes). The port file holds only their check values.
fn stock_sites(dev: Dev) -> std::collections::BTreeMap<u32, (u32, Vec<u8>)> {
    let out = std::process::Command::new(repo().join("dist/patchkit/patchkit"))
        .current_dir(repo())
        .args(["--root", ".", "port", "stock", "--firmware"])
        .arg(repo().join(dev.firmware))
        .arg("--port")
        .arg(repo().join(std::format!("devices/ports/{}.toml", dev.port())))
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
fn table_of(f: &[u8]) -> Vec<u8> {
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

/// The committed runtime object (its sections, joined): `cmpi.l #SVC_ABI,d0` of `abi_ok` is `0c80` and the word. A host build of the runtime has another `SVC_ABI`
/// (64-bit sizes), so the runtime of the device is the object.
fn runtime_object() -> String {
    let json = read(&repo().join("modules/core/runtime/build/runtime.dko.json"), "patchkit build modules/core/runtime");
    hex_sections(std::str::from_utf8(&json).unwrap()).join("|")
}

/// The registry of the device: the ABI check of the runtime object, the rows of the runtime's own registry.
struct DeviceRegistry {
    runtime: String,
}

impl Registry for DeviceRegistry {
    fn abi_ok(&self, fingerprint: u32) -> bool {
        fingerprint == 0 || self.runtime.contains(&std::format!("0c80{fingerprint:08x}"))
    }
    unsafe fn add(&mut self, rec: usize, row: Option<&ServiceRow>) -> i32 {
        RealRegistry.add(rec, row)
    }
    unsafe fn remove(&mut self, rec: usize) {
        // a module's registry rows are the scripted cleanup tests' (`registry_remove` asserts their event order); a provider's row is real
        crate::svc_rows::remove(rec)
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
            todo.extend(requires.get(&id).unwrap_or_else(|| panic!("{}: set {set} names {id}, which no module.toml under modules/ declares", dev.name)).clone());
            all.push(id);
        }
    }
    all
}

fn strings(v: &toml::Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap().to_string()).collect()).unwrap_or_default()
}

/// The `requires` of every module under `modules/`, by id (`[module] id` and `requires` of each `module.toml`).
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
    walk(&repo().join("modules"), &mut out);
    out
}

/// The module an SDK import needs (`requires` of its `[[import]]` in `sdk/sdk.toml`, the source of the runtime's `optional` link list); `None` for an import
/// without `requires`.
fn adapter_of(import: &str) -> Option<String> {
    let text = String::from_utf8(read(&repo().join("sdk/sdk.toml"), "git checkout sdk")).unwrap();
    let sdk: toml::Table = text.parse().unwrap();
    let imports = sdk["import"].as_array().unwrap();
    let row = imports.iter().find(|i| i["name"].as_str() == Some(import))?;
    row.get("requires").map(|r| r.as_str().unwrap().to_string())
}

/// The info table of a patched image (`set-<set>.info`): name, hash, kind (0 data, 1 slot, 2 DSP slot), address (the index in the slot block for a DSP slot) and the words of a data entry.
struct Image {
    entries: Vec<(String, u32, u32, u32, Vec<u32>)>,
    /// The modules of the set (`[patch_sets]` of the device file and what they require): the adapters the image has.
    modules: Vec<String>,
}

impl Image {
    fn read(dev: Dev, set: &str) -> Image {
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

    fn has(&self, hash: u32) -> bool {
        self.entries.iter().any(|e| e.1 == hash && !e.4.is_empty())
    }
}

fn cap_hash(kind: u8) -> u32 {
    CAP_INFO_HASHES[CAP_KINDS.iter().position(|k| *k == kind).unwrap()]
}

/// The capacity before anything loads, as `dkrt_capacity` builds it: the limits of the kinds the runtime counts itself are as large as they can be here (their
/// unit tests own them), the rest come from the image's `cap_<kind>` entries and the rows of the services loaded so far.
unsafe fn capacity(p: &FakePlatform) -> Vec<DkrtCap> {
    let table = crate::handlers::services();
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
            let v = crate::report::kind_value(p, &table, k, base);
            DkrtCap { kind: kind as u32, limit: v.1, used: v.2, largest: v.3 }
        })
        .collect()
}

// ---- the loader over a card ---------------------------------------------------------------------------------------------------

struct Nothing;
impl<C: dkrt_abi::core_api::CoreApi> Service<C> for Nothing {
    dkrt_abi::accepts_records!();
    dkrt_abi::accepts_teardown!();
}

/// The store and the arena of a card: every import resolves against the runtime's own tables (`RealImages`); `init` of a service file replays what its header
/// table declares (no ColdFire code runs on the host).
struct Card {
    files: Vec<(Vec<u8>, Vec<u8>)>,
    staged: Vec<u8>,
    staged_path: Vec<u8>,
    live: Vec<(u32, Vec<u128>)>,
    allocs: u32,
    /// The runtime id of the device whose `init` the card replays.
    device: u32,
}

impl Card {
    fn new(files: Vec<(String, Vec<u8>)>, device: u32) -> Card {
        Card {
            files: files.into_iter().map(|(n, b)| (n.into_bytes(), b)).collect(),
            staged: Vec::new(),
            staged_path: Vec::new(),
            live: Vec::new(),
            allocs: 0,
            device,
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
        Some(Block { ptr, addr })
    }
    unsafe fn free(&mut self, addr: u32) {
        let at = self.live.iter().position(|b| b.0 == addr).expect("free of a block that is not live");
        self.live.remove(at);
    }
    unsafe fn import_addr(&mut self, name: *const u8) -> u32 {
        // The device table `IMPORTS` points at firmware symbols that a host test binary does not link: a name the SDK declares gets a stand-in address.
        let n = core::slice::from_raw_parts(name, cstr_len(name, 64));
        if dkrt_sdk::api::BUILTIN_NAMES.iter().any(|b| b.as_bytes() == n) {
            0x1000
        } else {
            0
        }
    }
    unsafe fn svc_import_addr(&mut self, name: *const u8) -> u32 {
        RealImages.svc_import_addr(name)
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
        let f = &self.staged;
        if be(f, dkneeds::H_ROLE) != 1 {
            return OK; // a module's `init` is module code
        }
        let table = table_of(f);
        let keys = crate::collect::KeysCopy::from_bytes(&table).unwrap();
        let base = init - be(f, dkneeds::H_INIT);
        let at = |off: u32| (base + off) as usize as *const u8;
        let ctx = core::ptr::addr_of_mut!(crate::entry::DKRT_CTXS[rec]) as *mut u8;
        let mut kinds: Vec<(u32, u32)> = Vec::new();
        // the init of a service declares the hooks that its device has, with the bare entry: the packed table lists the hooks of every device, with a device mask in the entry
        let port = self.files.iter().find(|(n, _)| n.starts_with(b"ports/")).map(|(_, f)| table_of(f));
        let port = port.as_ref().map(|t| Keys::parse(t).unwrap());
        for i in 0..keys.decl.n {
            let mut d = keys.decl.items[i];
            if d.tag == Declare::HOOK {
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
                crate::collect::dk_service_declare_kind(ctx, d.a, d.b, at(d.c))
            } else if d.tag == Declare::SLOT {
                crate::collect::dk_service_declare_slot(ctx, d.a, d.b, at(d.c))
            } else if d.tag == Declare::HOOK {
                crate::collect::dk_service_declare_hook(ctx, d.a, at(d.b))
            } else if d.tag == Declare::EXPORT {
                crate::collect::dk_service_declare_export(ctx, d.a, at(d.b))
            } else {
                crate::collect::dk_service_declare_resource(ctx, d.a, d.b)
            };
            assert_eq!(r, OK);
        }
        // the row: the name of the file, the kinds of its table
        let stem = String::from_utf8(self.staged_path.clone()).unwrap();
        let stem = stem.trim_start_matches("services/").trim_end_matches(".dks").to_string();
        let name: &'static str = Box::leak(stem.into_boxed_str());
        let kinds: &'static [(u32, u32)] = Box::leak(kinds.into_boxed_slice());
        let row: &'static ServiceRow = Box::leak(Box::new(ServiceRow::of::<Nothing, crate::core_api::RealCore>(name, kinds)));
        crate::collect::dk_service_declare_row(ctx, row)
    }
    unsafe fn enter_fini(&mut self, _: usize, _: u32, _: *const Ctx) {}
}

type Boot = Loader<FakePlatform, Card, DeviceRegistry>;

/// The runtime tables of the host tests as a device has them before the boot loads anything: no loaded service (the other tests of the runtime run with the
/// machines row of `svc_rows::test_machines`; it is put back when the guard goes), no module, no DSP load.
struct Clean(std::sync::MutexGuard<'static, ()>);

fn clean() -> Clean {
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    unsafe {
        let _ = crate::handlers::services(); // the first call adds the test row
        for rec in MAX_MODULES..MAX_MODULES + PROVIDER_MAX {
            crate::svc_rows::remove(rec);
        }
        *st() = core::mem::zeroed();
        records::reset();
        *dsp::ds() = core::mem::zeroed();
        dsp::init_once();
        CALLED_NEED = [0; NEEDS_LEN];
        // the link script is a static: a test with no DSP part never calls `script_link`, and would see the strict tick drop of the initial value or the state of
        // whichever test ran before it
        crate::cleanup_tests::lenient_tick_drop();
    }
    Clean(g)
}

impl Drop for Clean {
    fn drop(&mut self) {
        unsafe {
            for rec in MAX_MODULES..MAX_MODULES + PROVIDER_MAX {
                crate::svc_rows::remove(rec);
            }
            crate::svc_rows::add(MAX_MODULES + PROVIDER_MAX - 1, crate::handlers::exports::machines_row());
        }
    }
}

/// Where a fixture's source module is built (`<project id>:pack`) and its file name.
struct Fixture {
    leaf: &'static str,
    nx: &'static str,
}

const SINE: Fixture = Fixture { leaf: "sine", nx: "test/sine" };
const DYN_DN2: Fixture = Fixture { leaf: "dyn-dn2", nx: "test/dyn-dn2" };
const FX_GAIN: Fixture = Fixture { leaf: "dyn-fx-gain", nx: "test/dyn-fx-gain" };
const FILTER_GAIN: Fixture = Fixture { leaf: "filter-gain", nx: "test/filter-gain" };
const LFO_SHAPE: Fixture = Fixture { leaf: "lfo-shape", nx: "test/lfo-shape" };
const SVC_USER: Fixture = Fixture { leaf: "svc-test-user", nx: "test/svc-test-user" };
/// A module that binds a key combination with `dk_shortcut_add`: a user of the shortcuts service file, no DSP section.
const SHORTCUT_DEMO: Fixture = Fixture { leaf: "shortcut-demo", nx: "test/shortcut-demo" };
/// A module that calls `dk_sample_pool_open` and the pool calls without owning a table: a user of the samples service file, no DSP section.
const TRACK_SELECT: Fixture = Fixture { leaf: "track-select", nx: "test/track-select" };

/// The service files of the card: the services of the runtime and the test service.
const SERVICES: [Fixture; 6] = [
    Fixture { leaf: "fx", nx: "core/services/fx" },
    Fixture { leaf: "machines", nx: "core/services/machines" },
    Fixture { leaf: "lfo", nx: "core/services/lfo" },
    Fixture { leaf: "samples", nx: "core/services/samples" },
    Fixture { leaf: "svc-test", nx: "test/svc-test" },
    Fixture { leaf: "shortcuts", nx: "core/services/shortcuts" },
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

/// The plan of a boot of `modules` from a card with `services`, over the platform of `image`: the loader with every module captured and the plan made. The
/// providers are not loaded yet.
unsafe fn planned(dev: Dev, image: &Image, limits: &[(u8, u32)], modules: &[&Fixture], services: &[Fixture]) -> (Boot, Plan) {
    let files = card_of(dev, modules, services);
    let p = image.platform(dev, limits);
    // the stock bytes of every hook site are live in the firmware (the check values of the port file are of them)
    for (addr, bytes) in stock_sites(dev).values() {
        p.put_code(*addr, bytes);
    }
    let mut l = Loader::new(p, Card::new(files, dev.profile.id), DeviceRegistry { runtime: runtime_object() });
    l.reset_plan();
    for (k, m) in modules.iter().enumerate() {
        l.capture(k, &packed(dev, m.leaf, "dkm", m.nx));
    }
    let caps = capacity(&l.p);
    let plan = l.plan_with_caps(modules.len(), &caps);
    (l, plan)
}

/// The store paths of the providers the plan found.
fn found(l: &Boot, plan: &Plan) -> BTreeSet<String> {
    (0..plan.np).map(|j| String::from_utf8(l.found.path(j).to_vec()).unwrap()).collect()
}

/// The hook sites (hook name hash, address) that the service files which loaded install: each `HOOKS` item of the packed file that the port file lists as present.
fn binds_of(dev: Dev, l: &Boot, plan: &Plan) -> Vec<(u32, u32)> {
    let pf = port_file(dev);
    let pt = table_of(&pf);
    let port = Keys::parse(&pt).unwrap();
    let mut out = Vec::new();
    for j in 0..plan.np {
        let path = String::from_utf8(l.found.path(j).to_vec()).unwrap();
        if !path.starts_with("services/") || !l.loaded[j] {
            continue;
        }
        let (_, file) = l.m.files.iter().find(|(n, _)| *n == path.as_bytes()).unwrap();
        let t = table_of(file);
        let keys = Keys::parse(&t).unwrap();
        for i in 0..keys.count(tag::HOOKS) {
            let hash = keys.words(tag::HOOKS, i).unwrap().0;
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
        let mut a = l.m.import_addr(name.as_ptr());
        if a == 0 && role == 1 {
            a = l.m.svc_import_addr(name.as_ptr());
        }
        if a == 0 {
            a = l.exports.resolve(name_hash(std::str::from_utf8(&name[..len]).unwrap())).unwrap_or(0);
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
    let src = std::fs::read_to_string(repo().join(std::format!("modules/core/adapters/dyndsp/src/cf/dyndsp-{}.s", dev.name))).unwrap();
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
struct DspEnv {
    _info: Vec<u32>,
    _guard: crate::cleanup_tests::VoiceInfoGuard,
}

impl Drop for DspEnv {
    fn drop(&mut self) {
        unsafe { crate::cleanup_tests::script_end() }
    }
}

unsafe fn dsp_env(dev: Dev, l: &Boot, plan: &Plan) -> Option<DspEnv> {
    let info = dsp_info_of(dev, l, plan)?;
    let guard = crate::cleanup_tests::VoiceInfoGuard::set(info.as_ptr());
    crate::cleanup_tests::script_link(true);
    Some(DspEnv { _info: info, _guard: guard })
}

/// The seam of a fixture with its provider: the plan accepts the module and finds `want` on the card, the providers load (the ABI word, the stock bytes of every
/// hook site), their hooks and slots are installed as their bind records say, and a module without a DSP section loads.
unsafe fn fits(dev: Dev, set: &str, modules: &[&Fixture], want: &[&str]) {
    let image = Image::read(dev, set);
    let (mut l, mut plan) = planned(dev, &image, &[], modules, &SERVICES);
    let what = std::format!("{} {set} {:?}", dev.name, modules.iter().map(|m| m.leaf).collect::<Vec<_>>());
    for k in 0..modules.len() {
        assert_eq!(plan.verdicts[k], Verdict::Accepted, "{what}: {}", modules[k].leaf);
    }
    let got = found(&l, &plan);
    for w in want {
        assert!(got.contains(&std::format!("services/{w}.dks")), "{what}: the search found {got:?}, not {w}");
    }
    for j in 0..plan.np {
        assert_eq!(plan.provider_verdicts[j], Verdict::Accepted, "{what}: provider {j}");
    }
    // a provider with a DSP part (the machines service) reaches the DSP loader from its load: the host gives it the import table of `core/dyndsp` (every name of its
    // sources, an address that is not 0) and a link that acknowledges, and stops there (the DSP code and the slots follow in the UI loop, `dsp_provider_tests`)
    let _dsp = dsp_env(dev, &l, &plan);
    l.load_providers(&plan);
    for j in 0..plan.np {
        assert!(l.loaded[j], "{what}: provider {} did not load", String::from_utf8_lossy(l.found.path(j)));
    }
    l.settle(&mut plan, modules.len());
    for k in 0..modules.len() {
        assert_eq!(plan.verdicts[k], Verdict::Accepted, "{what}: {} after the providers loaded", modules[k].leaf);
    }
    // the rows the files declared are in the table of the services
    let rows: Vec<&str> = crate::svc_rows::rows().iter().map(|r| unsafe { (**r).name() }).collect();
    for w in want {
        assert!(rows.contains(w), "{what}: no row {w} in {rows:?}");
    }
    // the hook sites of the files: one `jmp` written for each hook of a service file that the port lists as present, at its address, under one mask
    let binds = binds_of(dev, &l, &plan);
    assert_eq!(l.installer.count(), binds.len(), "{what}: installed sites");
    let written: Vec<u32> = l.p.calls.borrow().iter().filter_map(|c| if let Call::Write { addr, .. } = c { Some(*addr) } else { None }).collect();
    let mut sites: Vec<u32> = binds.iter().map(|b| b.1).collect();
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
            l.phase = Phase::Boot;
            let path = std::format!("modules/{}.dkm\0", m.leaf);
            let r = l.load(path.as_ptr());
            assert_eq!(r, OK, "{what}: load of {} (module {k})", m.leaf);
        }
    }
}

// ---- the cases ------------------------------------------------------------------------------------------------------------

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

/// A filter module (E7a) loads with the machines service file: the plan accepts its FILTER key and its NEEDS (the builder calls it imports are exports of the service file),
/// and the file's hooks of the filter (accessor, name, chooser list, loader byte) are installed at the sites its port file lists. The host cannot run the module's `init`, so
/// the rows it makes are the contract suite's (`modules/core/services/machines/tests/contract.rs`).
#[test]
fn a_filter_loads_with_the_machines_service_file() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        unsafe {
            fits(*dev, "emulator", &[&FILTER_GAIN], &["machines"]);
        }
        let _ = clean_state();
    }
}

/// Run the real packed filter through the DSP loader and the machines service's
/// reservation. The host supplies the sticky selection and the acknowledged link.
#[test]
fn filter_gain_reserves_one_state_per_voice_and_rejects_a_bad_callback() {
    use dkrt_abi::dsp::{I_AREA, I_AREA_BYTES, I_DEVICE, I_MAGIC, INFO_MAGIC, L2_BYTE_BASE, L2_SW_BASE};
    use dkrt_svc_machines::{dsp::filter_rows, filter};
    let _guard = clean();
    for dev in DEVICES {
        let mut file = packed(dev, FILTER_GAIN.leaf, "dkm", FILTER_GAIN.nx);
        let map = std::fs::read_to_string(repo().join(format!("dist/modules/{}/filter-gain.addrmap.json", dev.name))).unwrap();
        let off = be(&file, dkneeds::H_DSP_OFF);
        let len = be(&file, dkneeds::H_DSP_SIZE);
        let reg = be(&file, dkneeds::H_REG_OFF) as usize;
        let reg_len = be(&file, dkneeds::H_REG_SIZE) as usize;
        let mut keys = [0; 8];
        assert_eq!(dkrt_abi::keys::keys_of(&file[reg..reg + reg_len], dkneeds::KIND_FILTER, &mut keys), 1);
        let core = dkrt_test_support::FakeCore::new();
        core.boot_index.set(0);
        core.set_keys(0, dkneeds::KIND_FILTER, &keys[..1]);
        let mut sel = dkrt_abi::service::SettleCtx::new();
        sel.count = 1;
        let mut info = [0; dkrt_abi::dsp::I_IMPORTS];
        info[I_MAGIC] = INFO_MAGIC;
        info[I_DEVICE] = dev.profile.id;
        info[I_AREA] = L2_BYTE_BASE + 0x1000;
        info[I_AREA_BYTES] = 65536;
        unsafe {
            let _info = crate::cleanup_tests::VoiceInfoGuard::set(info.as_ptr());
            crate::cleanup_tests::script_link(true);
            crate::svc_rows::add(MAX_MODULES + PROVIDER_MAX - 1, crate::handlers::exports::machines_row());
            filter::plan(&core, &sel);
            let saved_index = crate::selection::BOOT_IDX;
            crate::selection::BOOT_IDX = 0;
            let mut job = dsp::Job::empty();
            assert_eq!(dsp::check(file.as_ptr(), off, len, file.len() as u32, &mut job), OK, "{}", dev.name);
            assert_eq!((job.nfilter, job.state_bytes), (1, 32 * 8));
            assert_eq!(dsp::reserve(0, &job), OK, "{}", dev.name);
            let index = (filter::resolve(&core, keys[0]) - filter::FIRST) as usize;
            let r = filter_rows::row(index);
            let pc = |symbol| L2_SW_BASE + (0x1000 + crate::contract_tests::symbol_offset(&map, symbol)) / 2;
            assert_eq!((r.coef_pc, r.process_pc), (pc("_fgan_coef"), pc("_fgan_process")));
            assert_eq!(r.stride, 8);
            assert_eq!(r.state, info[I_AREA] + ((job.image_size + job.bss + 7) & !7));
            let desc = job.image.add((r.desc - info[I_AREA]) as usize);
            assert_eq!(dkrt_abi::dsp::rd32le(desc.add(12)), 4);
            assert_eq!(dkneeds::needs_of(&mut file.as_slice()).unwrap().dsp_area, dsp::ds().mod_len[0]);
            let table = dkneeds::filter::contract::DK_FILTER_LINK_WORD + index as u32 * 4;
            for (step, (offset, value)) in [(1, r.process_pc), (2, r.state), (3, r.stride), (0, r.coef_pc)].into_iter().enumerate() {
                let mut want = step as u32;
                let w = crate::handlers::services().dsp_row_step(0, &mut want, true).unwrap();
                assert_eq!((w.win, w.word, w.value), (dkrt_abi::dsp::WIN_VOICES, table + offset, value));
                assert_eq!((w.last, w.has_undo, w.published.is_some()), (offset == 0, offset == 0, offset == 0));
                if let Some(mark) = w.published {
                    assert_eq!(w.undo, 0);
                    *mark = 1;
                }
            }
            let mut end = 4;
            assert!(crate::handlers::services().dsp_row_step(0, &mut end, true).is_none());
            assert!(dsp::quiesce(0));
            dsp::unreserve(0);
            assert_eq!(filter_rows::row(index).owner, 0);
            // Mutate copies of the actual packed section at the loader boundary.
            let check_bad = |bad: &[u8]| dsp::check(bad.as_ptr(), off, len, bad.len() as u32, &mut dsp::Job::empty());
            let set_word = |bad: &mut [u8], field: u32, value: u32| {
                let at = (off + field) as usize;
                bad[at..at + 4].copy_from_slice(&value.to_be_bytes());
            };
            let mut bad = file.clone();
            set_word(&mut bad, dkneeds::DSP_H_VOICE_N, u32::MAX);
            assert_eq!(check_bad(&bad), E_DSP_FORMAT);
            bad.clone_from(&file);
            set_word(&mut bad, dkneeds::DSP_H_BANK_OFF, len + 4);
            assert_eq!(check_bad(&bad), E_DSP_FORMAT);
            let desc_offset = r.desc - info[I_AREA];
            let reloc = be(&file, off + dkneeds::DSP_H_RELOC_OFF);
            assert!(reloc + dkneeds::RELOC_LEN <= be(&file, off + dkneeds::DSP_H_IMAGE_OFF));
            bad.clone_from(&file);
            set_word(&mut bad, dkneeds::DSP_H_RELOC_N, 1);
            set_word(&mut bad, reloc, desc_offset);
            assert_eq!(check_bad(&bad), E_DSP_FORMAT);
            bad.clone_from(&file);
            let state_word = (off + be(&file, off + dkneeds::DSP_H_IMAGE_OFF) + desc_offset + 12) as usize;
            bad[state_word..state_word + 4].copy_from_slice(&dkneeds::MAX_STATE_BYTES.to_le_bytes());
            assert_eq!(check_bad(&bad), E_DSP_NOSPACE);
            let process = (off + dkneeds::filter::TABLE + 8) as usize;
            file[process..process + 4].copy_from_slice(&job.image_size.to_be_bytes());
            assert_eq!(dsp::check(file.as_ptr(), off, len, file.len() as u32, &mut job), E_DSP_FORMAT);
            crate::selection::BOOT_IDX = saved_index;
            crate::cleanup_tests::script_end();
            clean_state();
        }
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

#[test]
fn an_lfo_loads_with_the_lfo_service_file() {
    let _g = clean();
    // one file for every device: each device above loads the same bytes (the fills of the other device's slots carry a device mask that the loader skips)
    let files: Vec<Vec<u8>> = DEVICES.iter().map(|d| service_file(*d, "lfo", "core/services/lfo")).collect();
    assert!(files.len() > 1 && files.iter().all(|f| *f == files[0]), "lfo.dks differs between devices");
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        // the image half (core/lfo) has the six slots of the device, kind 1 in the info table; `fits` loads the file, whose declared fills must name them, and installs each
        let image = Image::read(*dev, "emulator");
        for site in ["tick_call", "tick_slew", "tick_start", "read_call", "read_slew", "read_start"] {
            let name = std::format!("lfo_{}_{site}", dev.name);
            assert!(image.entries.iter().any(|e| e.0 == name && e.2 == 1), "{}: no slot {name} in the image", dev.name);
        }
        unsafe {
            fits(*dev, "emulator", &[&LFO_SHAPE], &["lfo"]);
        }
        let _ = clean_state();
        // the UI-task hooks: the port file lists each hook the lfo service takes and the device has (the name formatter and the page cell's vector pick; the Digitone II also its
        // three-character formatter), over the stock bytes of the firmware: `link.w A6,-0x1c; moveq 6,D0`. The check value of each is what the loader compares at the install.
        let file = service_file(*dev, "lfo", "core/services/lfo");
        let table = table_of(&file);
        let keys = Keys::parse(&table).unwrap();
        let pf = port_file(*dev);
        let pt = table_of(&pf);
        let port = Keys::parse(&pt).unwrap();
        let stock = stock_sites(*dev);
        let mut got = Vec::new();
        for i in 0..keys.count(tag::HOOKS) {
            let hash = keys.words(tag::HOOKS, i).unwrap().0;
            if port.find_site(hash).is_some_and(|s| s.flags & dkneeds::keys::FLAG_ABSENT == 0) {
                got.push((hash, stock[&hash].1.clone()));
            }
        }
        let name = |h: &str| name_hash(std::format!("cf.lfo.hook.{h}").as_str());
        let mut want = std::vec![
            (name("wave_name"), std::vec![0x4e, 0x56, 0xff, 0xe4, 0x70, 0x06]),
            // the vector pick of the page's cell: `addi.l #TABLE,D6` with the base of the firmware's RAM table of tile vectors (the hook's tail replays it)
            (
                name("graph_vec"),
                [0x06u8, 0x86]
                    .into_iter()
                    .chain(if dev.name == "dn2" { 0x4450_8b68u32 } else { 0x44d2_e37cu32 }.to_be_bytes())
                    .collect(),
            ),
        ];
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

/// The reload seam (docs/plans/extension-model.md E5): a real packed module of the boot is checked against the live set, taken down and loaded again into its
/// record, twice; a service file at a module's path is refused by the role check before anything is unloaded.
#[test]
fn a_packed_module_is_reloaded_twice_through_the_plan_and_a_service_file_is_refused() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        unsafe {
            let (mut l, mut plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let _dsp = dsp_env(*dev, &l, &plan);
            l.load_providers(&plan);
            l.settle(&mut plan, 1);
            let path = "modules/lfo-shape.dkm\0";
            l.phase = Phase::Boot;
            crate::selection::BOOT_IDX = 0;
            assert_eq!(l.load(path.as_ptr()), OK, "{}", dev.name);
            crate::selection::BOOT_IDX = u32::MAX;
            l.phase = Phase::Closed;
            let mut caps = capacity(&l.p);
            crate::loader::live_caps(&mut caps, crate::loader::own_use(0));
            // a service file is no module: refused, and the module is as it was
            assert_eq!(l.reload_check(0, "services/lfo.dks\0".as_ptr(), &caps), E_FORMAT, "{}", dev.name);
            assert_eq!(records::module(0).used, 1);
            for round in 0..2 {
                assert_eq!(l.reload_check(0, path.as_ptr(), &caps), OK, "{} round {round}", dev.name);
                assert_eq!(l.unload_rec(0), OK, "{} round {round}", dev.name);
                assert_eq!(l.reload_load(0, path.as_ptr()), OK, "{} round {round}", dev.name);
                assert_eq!((records::module(0).used, l.sel_of(0)), (1, 0));
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
            let (mut l, mut plan) = planned(*dev, &image, &[], &[&LFO_SHAPE], &SERVICES);
            let _dsp = dsp_env(*dev, &l, &plan);
            l.load_providers(&plan);
            l.settle(&mut plan, 1);
            let (path, copy) = ("modules/lfo-shape.dkm\0", "modules/lfo-shape-copy.dkm\0");
            let bytes = l.m.files.iter().find(|(n, _)| n == b"modules/lfo-shape.dkm").expect("the module is on the card").1.clone();
            l.m.files.push((b"modules/lfo-shape-copy.dkm".to_vec(), bytes));
            l.phase = Phase::Boot;
            crate::selection::BOOT_IDX = 0;
            assert_eq!(l.load(path.as_ptr()), OK, "{}", dev.name);
            crate::selection::BOOT_IDX = u32::MAX;
            l.phase = Phase::Closed;
            let caps = |l: &Boot| {
                let mut c = capacity(&l.p);
                crate::loader::live_caps(&mut c, (0, 0));
                c
            };
            // the module of that name is loaded: its copy is refused by name, before anything changes
            assert_eq!(l.live_check_load(1, copy.as_ptr(), &caps(&l)), E_LOADED, "{}", dev.name);
            assert_eq!((records::module(1).used, l.accepted[1]), (0, false));
            // an unload, then the copy at a new index (its file name was not in the boot's selection)
            assert_eq!(l.unload_rec(0), OK, "{}", dev.name);
            l.retire_index(0);
            assert_eq!((records::module(0).used, l.accepted[0], l.sel_of(0)), (0, false, u8::MAX), "{}", dev.name);
            assert_eq!(l.live_check_load(1, copy.as_ptr(), &caps(&l)), OK, "{}", dev.name);
            assert_eq!(l.live_load(1, copy.as_ptr()), OK, "{}", dev.name);
            assert_eq!((records::module(0).used, l.sel_of(0), l.accepted[1]), (1, 1, true), "{}", dev.name);
            // and down again, then the first path at its index
            assert_eq!(l.unload_rec(0), OK, "{}", dev.name);
            l.retire_index(1);
            assert_eq!(l.live_check_load(0, path.as_ptr(), &caps(&l)), OK, "{}", dev.name);
            assert_eq!(l.live_load(0, path.as_ptr()), OK, "{}", dev.name);
            assert_eq!((records::module(0).used, l.sel_of(0), l.accepted[0], l.accepted[1]), (1, 0, true, false), "{}", dev.name);
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
            let (_l, plan) = planned(*dev, &image, &[], &[&TRACK_SELECT], &[]);
            assert_ne!(plan.verdicts[0], Verdict::Accepted, "{}: the module needs a service the card does not hold", dev.name);
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
            let (mut l, mut plan) = planned(*dev, &image, &[], &[&TRACK_SELECT], &SERVICES);
            // the plan finds the file and cannot tell what the image lacks: the file fails its own check (its `requires` word) and the module then misses its service
            assert_eq!(plan.np, 1);
            l.load_providers(&plan);
            assert!(!l.loaded[0], "{}: samples.dks loaded without the sample-pool adapter", dev.name);
            l.settle(&mut plan, 1);
            assert!(matches!(plan.verdicts[0], Verdict::Refused(Reason::NeedMissing(_))), "{}: {:?}", dev.name, plan.verdicts[0]);
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
            l.load_providers(&plan);
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
                assert!(bare.info(name_hash("dyndsp_info")).is_null() && bare.info(name_hash("dynfx_info")).is_null());
                let full = image.platform(*dev, &[]);
                assert!(!full.info(name_hash("dyndsp_info")).is_null() && !full.info(name_hash("dynfx_info")).is_null());
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
            let mut modules: Vec<&Fixture> = std::vec![&SINE, &FX_GAIN, &LFO_SHAPE, &SVC_USER, &SHORTCUT_DEMO];
            let mut want = std::vec!["machines", "fx", "lfo", "svc-test", "shortcuts"];
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
            let (_l, plan) = planned(*dev, &image, &[(CAPK_MACHINE_ROWS, 0)], &[&SINE], &SERVICES);
            assert_eq!(plan.verdicts[0], Verdict::Refused(Reason::Fit(CAPK_MACHINE_ROWS)), "{}", dev.name);
            assert_eq!(plan.verdicts[0].result(), E_PLAN_FIT);
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
            let (_l, plan) = planned(*dev, &image, &[], &[&SINE], &SERVICES[..1]); // fx only
            assert_eq!(plan.verdicts[0], Verdict::Refused(Reason::NeedMissing(name_hash("machines"))), "{}", dev.name);
            assert_eq!(plan.verdicts[0].result(), E_MISSING);
        }
        let _ = clean_state();
    }
}

/// One key rule for every kind: the same packed module twice has the same machine key, so the boot plan refuses the second with the kind and the key that the first
/// has (`E_PLAN_KEY`), on both devices. A different module with other keys is accepted next to the first.
#[test]
fn the_same_machine_key_in_two_packed_modules_is_refused_by_the_boot_plan() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        let table = table_of(&packed(*dev, SINE.leaf, "dkm", SINE.nx));
        let key = Keys::parse(&table).unwrap().keys_of(dkneeds::KIND_MACHINE).next().expect("the sine fixture has a machine key");
        unsafe {
            let (_l, plan) = planned(*dev, &image, &[], &[&SINE, &SINE], &SERVICES[1..2]); // machines only
            assert_eq!(plan.verdicts[0], Verdict::Accepted, "{}", dev.name);
            assert_eq!(plan.verdicts[1], Verdict::Refused(Reason::DuplicateKey(dkneeds::KIND_MACHINE, key)), "{}", dev.name);
            assert_eq!(plan.verdicts[1].result(), dkrt_sdk::E_PLAN_KEY);
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
            let (_l, plan) = planned(*dev, &image, &[], &[&FX_GAIN], &SERVICES[1..]); // machines, lfo and the test service
            assert_eq!(plan.verdicts[0], Verdict::Refused(Reason::NeedMissing(name_hash("fx"))), "{}", dev.name);
            assert_eq!(plan.verdicts[0].result(), E_MISSING);
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
        // the one hook it takes: the entry of the key walker (devices/fw/input.toml), listed by the port file of the device and present on it
        let hooks: Vec<u32> = (0..keys.count(tag::HOOKS)).map(|i| keys.words(tag::HOOKS, i).unwrap().0).collect();
        assert_eq!(hooks, [name_hash("cf.input.hook.key_event")], "{}: the hooks shortcuts.dks declares", dev.name);
        let image = Image::read(*dev, "emulator");
        unsafe {
            // with the file on the card the module is accepted and the file is the provider that loads, and its builder import is one of its exports
            let (mut l, mut plan) = planned(*dev, &image, &[], &[&SHORTCUT_DEMO], &SERVICES[5..]);
            assert_eq!(plan.verdicts[0], Verdict::Accepted, "{}", dev.name);
            assert!(found(&l, &plan).contains("services/shortcuts.dks"), "{}", dev.name);
            l.load_providers(&plan);
            l.settle(&mut plan, 1);
            // the hook site is one `jmp` at the walker entry of the device's firmware
            let binds = binds_of(*dev, &l, &plan);
            assert_eq!(binds.len(), 1, "{}: the sites shortcuts.dks installs", dev.name);
            assert_eq!(binds[0].0, name_hash("cf.input.hook.key_event"), "{}", dev.name);
            assert!(l.exports.resolve(name_hash("dk_shortcut_add")).is_some(), "{}: dk_shortcut_add is no export of shortcuts.dks", dev.name);
            let listed: Vec<u32> = (0..keys.count(tag::EXPORTS)).map(|i| keys.words(tag::EXPORTS, i).unwrap().0).collect();
            assert_eq!(listed, [name_hash("dk_shortcut_add")], "{}: the header table's exports", dev.name);
            assert!(import_names(&packed(*dev, SHORTCUT_DEMO.leaf, "dkm", SHORTCUT_DEMO.nx)).iter().any(|n| n == "dk_shortcut_add"), "{}", dev.name);
        }
        let _ = clean_state();
        unsafe {
            // a card without the file refuses it, naming the service
            let (_l, plan) = planned(*dev, &image, &[], &[&SHORTCUT_DEMO], &SERVICES[..5]);
            assert_eq!(plan.verdicts[0], Verdict::Refused(Reason::NeedMissing(name_hash("shortcuts"))), "{}", dev.name);
            assert_eq!(plan.verdicts[0].result(), E_MISSING);
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
                l.load_providers(&plan);
                let file = packed(*dev, module.leaf, "dkm", module.nx);
                let used: Vec<String> = import_names(&file).into_iter().filter(|n| builders.contains(&n.as_str())).collect();
                assert!(used.len() >= 3, "{} {}: the module calls {used:?} of the {service} builders", dev.name, module.leaf);
                for n in &used {
                    assert!(l.exports.resolve(name_hash(n)).is_some(), "{} {}: {n} is no export of {service}.dks", dev.name, module.leaf);
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

/// The header EXPORTS of a service file are exactly its `provided` imports of sdk/sdk.toml, by name hash (the file's `exports()` is generated from the same list): the
/// builder calls of the modules, and the `internal` ones that only the runtime calls (`handlers::exports` names them, so a renamed export is a failure here).
#[test]
fn the_header_exports_of_a_service_file_are_its_provided_imports() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for service in ["machines", "fx", "samples"] {
            let owner = service_file(*dev, service, &format!("core/services/{service}"));
            let table = table_of(&owner);
            let keys = Keys::parse(&table).unwrap();
            let listed: std::collections::BTreeSet<u32> = (0..keys.count(tag::EXPORTS)).map(|i| keys.words(tag::EXPORTS, i).unwrap().0).collect();
            use crate::handlers::exports::{FX_ROUTES_ENTRY, MACHINES_MODULE_INFO};
            let internal: &[&str] = match service {
                "fx" => &[FX_ROUTES_ENTRY],
                "machines" => &[MACHINES_MODULE_INFO],
                _ => &[],
            };
            let want: std::collections::BTreeSet<u32> = builders_of(service).into_iter().chain(internal.iter().copied()).map(name_hash).collect();
            assert!(!want.is_empty(), "{service} provides nothing in sdk/sdk.toml");
            assert_eq!(listed, want, "{} {service}.dks: header EXPORTS differ from the provided imports", dev.name);
        }
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
            let caps = capacity(&l.p);
            let plan2 = l.plan_with_caps(1, &caps);
            assert!(!(0..plan2.np).any(|j| l.found.path(j).starts_with(b"ports/")), "{}: a port file for another firmware was found", dev.name);
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

/// The committed objects of the service files carry the ABI word the runtime object compares with (`dk_svc_abi` against the immediate of `abi_ok`), and the packed
/// file of each device has it in header word 0x3c. A source change that moves `SVC_ABI` leaves a service object stale: the loader then refuses every service file.
#[test]
fn the_service_objects_and_files_carry_the_abi_word_of_the_runtime_object() {
    let runtime = runtime_object();
    for s in SERVICES.iter().filter(|s| s.nx.starts_with("core/") && s.leaf != "samples") {
        let json = read(&repo().join(format!("modules/core/services/{}/dks/build/{}.dko.json", s.leaf, s.leaf)), "patchkit build modules/core");
        let abi = object_abi(std::str::from_utf8(&json).unwrap()).to_string();
        assert!(
            runtime.contains(&std::format!("0c80{abi}")),
            "{}.dko.json carries ABI word {abi}, the runtime object another: patchkit build modules/core/services/{}/dks",
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
        if tag == Declare::HOOK || tag == Declare::EXPORT {
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
    let services: Vec<(&Fixture, crate::handlers::dks_decls::DeclareFn)> = crate::handlers::dks_decls::declarations()
        .iter()
        .map(|(leaf, declare)| (SERVICES.iter().find(|s| s.leaf == *leaf).unwrap(), *declare))
        .collect();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        for (s, declare) in &services {
            // the packed file carries the hooks of both devices (with a device mask in the entry) and the slot fills of both (with a device mask in the flags): the loader drops the masks, the hooks the port file lists as absent and the slots of the other device
            let file = service_file(*dev, s.leaf, s.nx);
            let header = crate::collect::KeysCopy::from_bytes(&table_of(&file)).unwrap().decl;
            let pf = port_file(*dev);
            let pt = table_of(&pf);
            let port = Keys::parse(&pt).unwrap();
            let mut want: Vec<(u16, u32, u32)> = (0..header.n)
                .filter(|i| {
                    let d = header.items[*i];
                    // a slot fill of another device's image is skipped by the loader
                    !(d.tag == Declare::HOOK && port.find_site(d.a).is_some_and(|x| x.flags & dkneeds::keys::FLAG_ABSENT != 0))
                        && !(d.tag == Declare::SLOT && !dkneeds::keys::slot_for_device(d.b, dev.profile.id))
                })
                .map(|i| plain(header.items[i].tag, header.items[i].a, header.items[i].b & !dkneeds::keys::SLOT_DEVICES_MASK))
                .collect();
            let mut got: Vec<(u16, u32, u32)> = Vec::new();
            unsafe {
                let mut col = std::boxed::Box::new(crate::collect::Collect::new());
                crate::collect::begin(&mut *col);
                let ctx = crate::entry::bind(0) as *mut u8;
                let r = declare(ctx, if dev.name == "dt2" { dt2 } else { dn2 });
                crate::collect::end();
                crate::entry::unbind(0);
                assert_eq!(r, OK, "{} {}: init refused its own declarations", dev.name, s.leaf);
                for i in 0..col.decl.n {
                    let d = col.decl.items[i];
                    if d.tag != Declare::ROW {
                        got.push(plain(d.tag, d.a, d.b));
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
fn clean_state() {
    unsafe {
        for rec in MAX_MODULES..MAX_MODULES + PROVIDER_MAX {
            crate::svc_rows::remove(rec);
        }
        *st() = core::mem::zeroed();
        records::reset();
        *dsp::ds() = core::mem::zeroed();
        dsp::init_once();
        CALLED_NEED = [0; NEEDS_LEN];
    }
}

/// The boot's own loop (`Loader::boot`) over a plan of real files: the accepted module loads in plan order and is reported to the boot record, the refused one is
/// reported with the planner's reason and detail words and its file is never staged.
#[test]
fn the_boot_loop_loads_accepted_modules_in_plan_order_and_reports_a_refused_one_without_loading_it() {
    let _g = clean();
    for dev in DEVICES.iter().filter(|d| d.has_firmware()) {
        let image = Image::read(*dev, "emulator");
        if be(&packed(*dev, SVC_USER.leaf, "dkm", SVC_USER.nx), dkneeds::H_DSP_SIZE) != 0 {
            eprintln!("SKIP {}: {} has a DSP section, which the host cannot load", dev.name, SVC_USER.leaf);
            continue;
        }
        unsafe {
            // a machine row limit of 0 refuses `sine` (the plan names the resource); `svc-test-user` needs no machine row
            let (mut l, mut plan) = planned(*dev, &image, &[(CAPK_MACHINE_ROWS, 0)], &[&SVC_USER, &SINE], &SERVICES);
            l.load_providers(&plan);
            l.settle(&mut plan, 2);
            assert_eq!(plan.verdicts[0], Verdict::Accepted, "{}", dev.name);
            assert_eq!(plan.verdicts[1], Verdict::Refused(Reason::Fit(CAPK_MACHINE_ROWS)), "{}", dev.name);

            // the boot record as `dkrt_boot` leaves it before the loop: the verdicts are in
            *crate::boot::info() = core::mem::zeroed();
            crate::boot::info().count = 2;
            crate::boot::states().begin();
            for k in 0..2 {
                crate::boot::states().planned(k, plan.verdicts[k]);
            }
            let mut rec = dkrt_sdk::sel::Record::empty();
            rec.count = 2;
            for (k, m) in [&SVC_USER, &SINE].iter().enumerate() {
                let path = std::format!("modules/{}.dkm", m.leaf);
                rec.paths[k][..path.len()].copy_from_slice(path.as_bytes());
            }
            l.boot(&rec, &plan);

            let b = crate::boot::info();
            assert_eq!(b.results[..2], [OK, E_PLAN_FIT], "{}", dev.name);
            assert_eq!(b.loaded, 1, "{}", dev.name);
            assert_eq!((b.states[0], b.states[1]), (STATE_ACTIVE, STATE_REFUSED), "{}", dev.name);
            assert_eq!(b.detail[1], [CAPK_MACHINE_ROWS as u32, 0], "{}", dev.name);
            // the accepted module is live in the first record; the refused file was never read from the store
            assert_eq!((st().modules[0].used, st().modules[1].used, st().loads), (1, 0, 1), "{}", dev.name);
            assert_eq!(l.m.staged_path, b"modules/svc-test-user.dkm".to_vec(), "{}: the last file staged", dev.name);
            // each result names its file's leaf
            let leaf = |k: usize| core::slice::from_raw_parts(crate::boot::boot_name(k), dkrt_abi::state::NAME_LEN).to_vec();
            assert_eq!(&leaf(0)[..17], b"svc-test-user.dkm");
            assert_eq!(&leaf(1)[..8], b"sine.dkm");
            // the loop leaves the loader closed
            assert!(l.phase == Phase::Closed);
            assert_eq!(l.load(b"modules/svc-test-user.dkm\0".as_ptr()), E_BOOT_ONLY);
        }
        let _ = clean_state();
    }
}
