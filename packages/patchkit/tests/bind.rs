//! `patchkit bind`: the service fixture is packed in memory (no toolchain, no firmware) and its `HOOKS` entry is checked; binding is checked against
//! the real stock images (skipped when the images are absent): the site, the stock bytes and the return address a service is bound with are the
//! ones `apply` finds for the same hook. The bound files stay in memory (a bound file holds firmware bytes). The pure parts of binding (trampoline
//! encoding, the limits, the bss shift) are unit tests in `src/bind.rs`.

use dkneeds::keys::{Keys, name_hash};
use patchkit::apply::{self, Options};
use patchkit::dkm::{Dkm, ROLE_SERVICE};
use patchkit::dko::{Dko, DkoReloc, DkoSection, DkoSymbol, FORMAT};
use patchkit::error::hex;
use patchkit::firmware::Firmware;
use patchkit::module::{Library, Module};
use patchkit::pack::{PackOptions, pack_module};
use patchkit::port::Registry;
use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn image(name: &str) -> Option<Vec<u8>> {
    std::fs::read(root().join("firmware").join(name)).ok()
}

const DT2: &str = "Digitakt_II_OS1.17.syx";
const DN2: &str = "Digitone_II_OS1.12.syx";

macro_rules! need_image {
    ($f:expr) => {
        if image($f).is_none() {
            return;
        }
    };
}

const HOOK: &str = "cf.menu.hook.settings_list.tail";

const SERVICE: &str = "[module]\nid = \"test/svc-test\"\nversion = \"0.8.2\"\nname = \"S\"\ncategory = \"test\"\nrole = \"service\"\nservice = \"svc-test\"\ntype = \"raw\"\nauthor = \"digikit\"\nstatus = \"experimental\"\nsummary = \"s\"\n[[hook]]\nname = \"cf.menu.hook.settings_list.tail\"\nentry = \"hooktest_settings_tail\"\n[[object]]\npath = \"svc.dko\"\nplace = \"cf.expansion\"\n";

fn sec(name: &str, kind: &str, align: u32, bytes: &[u8]) -> DkoSection {
    DkoSection {
        name: name.into(),
        kind: kind.into(),
        align,
        size: bytes.len() as u32,
        hex: hex(bytes),
    }
}

fn sym(name: &str, section: &str, value: u64, global: bool) -> DkoSymbol {
    DkoSymbol {
        name: name.into(),
        section: Some(section.into()),
        value,
        global,
    }
}

fn rel(offset: u64, symbol: &str) -> DkoReloc {
    DkoReloc {
        section: ".data".into(),
        offset,
        rtype: "cf.32".into(),
        symbol: symbol.into(),
        addend: 0,
    }
}

/// A ColdFire object with `dk_module` (init and tick set), the exports `dk_svctest_ping` and `dk_svctest_hits`, the global handler `hooktest_settings_tail` and the static `local_tail`.
fn service_object() -> Dko {
    let text = [
        0x4e, 0x75, 0x4e, 0x75, 0x4e, 0x75, 0x4e, 0x75, 0x4e, 0x75, 0x4e, 0x75,
    ];
    let mut o = Dko {
        format: FORMAT,
        arch: "cf".into(),
        source_sha256: "0".repeat(64),
        toolchain: Default::default(),
        sections: vec![
            sec(".text", "text", 2, &text),
            sec(".data", "data", 4, &[0; 24]),
        ],
        symbols: vec![
            sym("init_fn", ".text", 0, true),
            sym("tick_fn", ".text", 2, true),
            sym("hooktest_settings_tail", ".text", 6, true),
            sym("local_tail", ".text", 8, false),
            sym("dk_module", ".data", 8, true),
            // the exports of svc-test (its header lists the provided imports of sdk/sdk.toml)
            sym("dk_svctest_ping", ".text", 0, true),
            sym("dk_svctest_hits", ".text", 2, true),
        ],
        relocs: vec![rel(8, "init_fn"), rel(16, "tick_fn")],
    };
    o.stamp_isa();
    o
}

fn library(manifest: &str) -> Library {
    Library {
        modules: vec![Module {
            manifest: toml::from_str(manifest).unwrap(),
            objects: vec![service_object()],
            alloc: Default::default(),
            digest: String::new(),
            dir: Default::default(),
        }],
    }
}

fn pack_with(manifest: &str, device: &str) -> patchkit::Result<Vec<u8>> {
    let o = PackOptions {
        device: Some(device.into()),
        ..Default::default()
    };
    pack_module(&library(manifest), "test/svc-test", &o).map(|p| p.bytes)
}

/// The packed service file for `device` ("dt2" or "dn2").
fn pack_service(device: &str) -> Vec<u8> {
    pack_with(SERVICE, device).unwrap()
}

fn registry() -> Registry {
    Registry::from_dir(&root().join("devices/ports")).unwrap()
}

#[test]
fn a_service_with_a_hook_packs_its_hooks_entry() {
    let dks = pack_service("dt2");
    let dkm = Dkm::parse(&dks).unwrap();
    assert_eq!(dkm.role, ROLE_SERVICE);
    let k = Keys::parse(&dkm.register).unwrap();
    let hooks: Vec<_> = k.hooks().map(|p| (p.a, p.b)).collect();
    assert_eq!(hooks.len(), 1);
    assert_eq!(hooks[0].0, name_hash(HOOK));
    assert_eq!(
        hooks[0].1,
        patchkit::dkm::image_symbols(&service_object()).unwrap()["hooktest_settings_tail"]
    );
    assert_eq!(k.bind_count(), 0, "a packed service is not bound");
}

#[test]
fn the_packer_writes_the_device_mask_of_a_hook_in_the_top_byte() {
    let m = SERVICE.replace(
        "entry = \"hooktest_settings_tail\"\n",
        "entry = \"hooktest_settings_tail\"\ndevices = [\"dn2\"]\n",
    );
    let dks = pack_with(&m, "dt2").unwrap();
    let dkm = Dkm::parse(&dks).unwrap();
    let k = Keys::parse(&dkm.register).unwrap();
    let h = k.hooks().next().unwrap();
    let mask = patchkit::dkm::hook_device_mask(&["dn2".to_string()]).unwrap();
    assert_ne!(mask, 0);
    assert_eq!(h.b >> 24, mask);
    assert_eq!(
        h.b & patchkit::dkm::HOOK_ENTRY_MASK,
        patchkit::dkm::image_symbols(&service_object()).unwrap()["hooktest_settings_tail"]
    );
}

#[test]
fn a_missing_static_or_repeated_handler_and_a_hook_without_the_role_are_refused() {
    for bad in ["no_such_fn", "local_tail"] {
        let m = SERVICE.replace("hooktest_settings_tail", bad);
        let e = pack_with(&m, "dt2").unwrap_err().to_string();
        assert!(e.contains(bad), "{e}");
    }
    let twice = SERVICE.replace(
        "[[object]]",
        "[[hook]]\nname = \"cf.menu.hook.settings_list.tail\"\nentry = \"init_fn\"\n[[object]]",
    );
    let e = pack_with(&twice, "dt2").unwrap_err().to_string();
    assert!(e.contains("declared twice"), "{e}");
    let plain = SERVICE
        .replace("role = \"service\"\n", "")
        .replace("service = \"svc-test\"\n", "");
    let e = pack_with(&plain, "dt2").unwrap_err().to_string();
    assert!(
        e.contains("belong to a module with `role = \"service\"`"),
        "{e}"
    );
}

/// A ColdFire object with `rts; rts` and the global symbols `_f` (0) and `_g` (2), for the apply side.
fn apply_object() -> serde_json::Value {
    serde_json::json!({
        "format": 1, "arch": "cf", "source_sha256": "0".repeat(64),
        "sections": [{"name": ".text", "kind": "text", "align": 2, "size": 4, "hex": "4e754e75"}],
        "symbols": [
            {"name": "_f", "section": ".text", "value": 0, "global": true},
            {"name": "_g", "section": ".text", "value": 2, "global": true}
        ],
        "relocs": []
    })
}

fn apply_module() -> Module {
    let text = format!(
        "[module]\nid = \"t/h\"\nversion = \"1.0.0\"\nname = \"t/h\"\ncategory = \"test\"\ntype = \"raw\"\nauthor = \"digikit\"\nstatus = \"experimental\"\nsummary = \"synthetic\"\n\n[[object]]\npath = \"x.json\"\nplace = \"cf.expansion\"\n[[op]]\nop = \"hook\"\nhook = \"{HOOK}\"\nto = \"_f\"\n"
    );
    let read = |_p: &str| -> patchkit::Result<String> { Ok(apply_object().to_string()) };
    Module::from_parts(&text, &read).unwrap()
}

fn check(img: &str, device: &str, site: u64, ret: u64) {
    let raw = image(img).unwrap();
    let reg = registry();
    let texts = patchkit::fwgen::fw_texts(&root()).unwrap();
    let decls: Vec<&str> = texts.iter().map(String::as_str).collect();
    let dks = pack_service(device);
    let (file, hooks) = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap();
    assert_eq!(hooks.len(), 1);
    let b = &hooks[0];
    assert_eq!(b.hook, HOOK);

    // the bound file: one BIND item with the site hash, address, trampoline offset and stock bytes
    let dkm = Dkm::parse(&file).unwrap();
    assert_eq!(dkm.role, ROLE_SERVICE);
    let k = Keys::parse(&dkm.register).unwrap();
    assert_eq!(k.bind_count(), 1);
    let item = k.bind(0).unwrap();
    assert_eq!(item.site, name_hash(HOOK));
    assert_eq!(item.addr, b.site_addr);
    assert_eq!(item.len, b.site_len);
    assert_eq!(item.tramp, b.tramp_off);
    assert_eq!(item.stock, &b.stock[..]);
    assert!((item.tramp as usize) < dkm.image.len());
    assert!(
        k.hooks()
            .eq(Keys::parse(&Dkm::parse(&dks).unwrap().register)
                .unwrap()
                .hooks())
    );
    // binding twice is refused
    let e = patchkit::bind::bind(&file, &raw, &reg, &decls).unwrap_err();
    assert!(e.0.contains("already bound"), "{e}");

    let mut lib = Library::scan_repo_for(&root(), true, Some(&[])).unwrap();
    lib.modules.push(apply_module());
    let out =
        apply::apply_with(&raw, &reg, &lib, &["t/h".to_string()], Options::default()).unwrap();
    let j = serde_json::to_value(&out.plan).unwrap();
    let h = &j["hooks"][0];
    assert_eq!(h["hook"], HOOK);

    // the same site, length, return address and displaced instructions
    assert_eq!(b.site_addr as u64, h["addr"].as_u64().unwrap());
    assert_eq!(b.site_len as u64, h["site_len"].as_u64().unwrap());
    assert_eq!(b.ret as u64, h["ret"].as_u64().unwrap());
    let displaced: Vec<String> = h["displaced_insns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect();
    assert_eq!(b.displaced, displaced);
    assert_eq!(b.stock.len(), b.site_len as usize);
    let fw = Firmware::parse(&raw).unwrap();
    let main = fw.section(3).unwrap();
    let o = (b.site_addr as u64 - main.dest as u64) as usize;
    assert_eq!(&main.data[o..o + b.stock.len()], &b.stock[..]);

    // the values of the static gate
    assert_eq!(b.site_addr as u64, site);
    assert_eq!(b.ret as u64, ret);
    assert_eq!(hex(&b.stock), "205248780002");
}

#[test]
fn bind_and_apply_agree_on_the_dt2_site() {
    need_image!(DT2);
    check(DT2, "dt2", 0x400a_5eac, 0x400a_5eb2);
}

#[test]
fn bind_and_apply_agree_on_the_dn2_site() {
    need_image!(DN2);
    check(DN2, "dn2", 0x4009_ff88, 0x4009_ff8e);
}

// ---- raw hooks with a tail exit ------------------------------------------------------------------------------------

/// A raw hook (declared in `devices/fw/` with no handler, mode `replace`) on the DT2 image whose displaced instructions can be copied: its name, the declarations
/// with `tail = true` added to it and the resolved site.
fn raw_hook(raw: &[u8], reg: &Registry) -> (String, Vec<String>, patchkit::apply::HookSite) {
    let texts = patchkit::fwgen::fw_texts(&root()).unwrap();
    let f = Firmware::parse(raw).unwrap();
    let port = reg.find(&f.syx_sha256).unwrap();
    for h in port.hook.iter().filter(|h| h.name.starts_with("cf.")) {
        let Ok(site) = patchkit::apply::resolve_hook_site(raw, reg, &h.name) else {
            continue;
        };
        if site.mode != "replace" || site.old.len() > dkneeds::keys::BIND_STOCK_MAX {
            continue;
        }
        let a = patchkit::prim::cf_tail(&site.cf, 0, site.ret);
        let b = patchkit::prim::cf_tail(&site.cf, 0x1000, site.ret);
        if a.is_err() || a.ok() != b.ok() {
            continue;
        }
        let key = format!("name = \"{}\"\n", h.name);
        let Some(text) = texts
            .iter()
            .find(|t| t.contains(&key) && t.contains("[[hook]]"))
        else {
            continue;
        };
        // a hook that declares exits already is left for the real services (the test adds its own `exits`)
        let at = text.find(&key).unwrap();
        let block = &text[at..text[at..].find("[[").map_or(text.len(), |e| at + e)];
        if block.contains("exits =") {
            continue;
        }
        let mut decls = texts.clone();
        let i = decls.iter().position(|t| t == text).unwrap();
        decls[i] = text.replacen(&key, &format!("{key}tail = true\n"), 1);
        return (h.name.clone(), decls, site);
    }
    panic!("no raw hook with a copyable site on the image");
}

/// The service fixture taking the raw hook `hook`, whose object refers to the tail of the hook (`dk_tail_<hash>`) in a data word (after the four of `dk_module`).
fn tail_service(hook: &str, device: &str) -> Vec<u8> {
    let manifest = SERVICE.replace(HOOK, hook);
    let mut obj = service_object();
    obj.sections[1] = sec(".data", "data", 4, &[0; 28]);
    obj.relocs.push(rel(24, &patchkit::bind::tail_import(hook)));
    obj.stamp_isa();
    let lib = Library {
        modules: vec![Module {
            manifest: toml::from_str(&manifest).unwrap(),
            objects: vec![obj],
            alloc: Default::default(),
            digest: String::new(),
            dir: Default::default(),
        }],
    };
    let o = PackOptions {
        device: Some(device.into()),
        ..Default::default()
    };
    pack_module(&lib, "test/svc-test", &o).unwrap().bytes
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_raw_hook_with_a_tail_binds_to_the_stock_bytes_of_the_image() {
    need_image!(DT2);
    let raw = image(DT2).unwrap();
    let reg = registry();
    let (hook, decls, site) = raw_hook(&raw, &reg);
    let decls: Vec<&str> = decls.iter().map(String::as_str).collect();
    let dks = tail_service(&hook, "dt2");
    let (file, hooks) = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap();
    let b = &hooks[0];
    let tail_off = b.tail_off.expect("the raw hook has a tail") as usize;
    let dkm = Dkm::parse(&file).unwrap();
    // the BIND item carries the site's stock bytes; the trampoline is the entry itself
    let k = Keys::parse(&dkm.register).unwrap();
    let item = k.bind(0).unwrap();
    assert_eq!(item.stock, &site.old[..]);
    assert_eq!(
        item.tramp,
        patchkit::dkm::image_symbols(&service_object()).unwrap()["hooktest_settings_tail"]
    );
    // the tail is the displaced instructions of the user's image, then a jump to the first instruction after the site
    let want = patchkit::prim::cf_tail(&site.cf, 0, site.ret).unwrap();
    assert_eq!(&dkm.image[tail_off..tail_off + want.len()], &want[..]);
    // the reference is a base-relative relocation to the tail and the import is gone
    assert!(
        dkm.imports
            .iter()
            .all(|n| n != &patchkit::bind::tail_import(&hook))
    );
    assert!(dkm.relocs.iter().any(|(off, k)| {
        *off != 0
            && k.is_none()
            && u32::from_be_bytes(
                dkm.image[*off as usize..*off as usize + 4]
                    .try_into()
                    .unwrap(),
            ) as usize
                == tail_off
    }));
}

#[test]
fn an_unbound_dks_holds_no_firmware_bytes() {
    need_image!(DT2);
    let raw = image(DT2).unwrap();
    let reg = registry();
    let (hook, decls, site) = raw_hook(&raw, &reg);
    let decls: Vec<&str> = decls.iter().map(String::as_str).collect();
    let dks = tail_service(&hook, "dt2");
    let tail = patchkit::prim::cf_tail(&site.cf, 0, site.ret).unwrap();
    // unbound: the tail is a named import, and neither the site's stock bytes nor the tail code are anywhere in the file
    let dkm = Dkm::parse(&dks).unwrap();
    assert!(dkm.imports.contains(&patchkit::bind::tail_import(&hook)));
    assert!(
        !contains(&dks, &site.old),
        "stock bytes of the site in the unbound file"
    );
    assert!(!contains(&dks, &tail), "the tail code in the unbound file");
    // bound: both are there
    let (file, _) = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap();
    assert!(contains(&file, &site.old) && contains(&file, &tail));
}

/// `patchkit --root ROOT bind IN --firmware FW -o OUT`: the refusal comes before the firmware and the ports are read.
fn bind_cli(root: &Path, dks: &Path, out: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_patchkit"))
        .arg("--root")
        .arg(root)
        .arg("bind")
        .arg(dks)
        .arg("--firmware")
        .arg(root.join("no-such.syx"))
        .arg("-o")
        .arg(out)
        .output()
        .unwrap()
}

#[test]
fn the_command_refuses_an_output_in_the_source_tree_outside_dist_and_out() {
    let d = std::env::temp_dir().join(format!("pk-bind-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let dks = d.join("in.dks");
    std::fs::write(&dks, pack_service("dt2")).unwrap();
    for out in ["svc.dks", "modules/svc.dks"] {
        let r = bind_cli(&d, &dks, &d.join(out));
        assert!(!r.status.success(), "{out} must be refused");
        let e = String::from_utf8_lossy(&r.stderr);
        assert!(e.contains("must never be committed"), "{out}: {e}");
        assert!(!d.join(out).exists());
    }
    // under dist/ or out/ the check passes: the command goes on and stops at the missing ports
    for out in ["dist/svc.dks", "out/x/svc.dks"] {
        let r = bind_cli(&d, &dks, &d.join(out));
        assert!(!r.status.success());
        let e = String::from_utf8_lossy(&r.stderr);
        assert!(!e.contains("must never be committed"), "{out}: {e}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

// ---- firmware references: dk_fw_, dk_exit_, dk_stock_, dk_ret_ and the FW item ---------------------------------------------

/// The service fixture with the references `imports` in data words (after the four of `dk_module`); with `hook` it takes that raw hook, without it takes none.
fn refs_service(hook: Option<&str>, device: &str, imports: &[String]) -> Vec<u8> {
    let manifest = match hook {
        Some(h) => SERVICE.replace(HOOK, h),
        None => SERVICE.replace(
            "[[hook]]\nname = \"cf.menu.hook.settings_list.tail\"\nentry = \"hooktest_settings_tail\"\n",
            "",
        ),
    };
    let mut obj = service_object();
    obj.sections[1] = sec(".data", "data", 4, &vec![0; 24 + 4 * imports.len()]);
    for (i, n) in imports.iter().enumerate() {
        obj.relocs.push(rel(24 + 4 * i as u64, n));
    }
    obj.stamp_isa();
    let lib = Library {
        modules: vec![Module {
            manifest: toml::from_str(&manifest).unwrap(),
            objects: vec![obj],
            alloc: Default::default(),
            digest: String::new(),
            dir: Default::default(),
        }],
    };
    let o = PackOptions {
        device: Some(device.into()),
        ..Default::default()
    };
    pack_module(&lib, "test/svc-test", &o).unwrap().bytes
}

/// The image offset of the data section of the fixture (`dk_module` is 8 bytes into it).
fn data_base() -> usize {
    patchkit::dkm::image_symbols(&service_object()).unwrap()["dk_module"] as usize - 8
}

/// The word `at` bytes into the data section of the fixture.
fn word_at(dkm: &Dkm, at: usize) -> u32 {
    let at = data_base() + at;
    u32::from_be_bytes(dkm.image[at..at + 4].try_into().unwrap())
}

/// The first ColdFire code anchor of the port of `raw`: its name and address.
fn some_anchor(raw: &[u8], reg: &Registry) -> (String, u32) {
    let f = Firmware::parse(raw).unwrap();
    let port = reg.find(&f.syx_sha256).unwrap();
    let a = port
        .anchor
        .iter()
        .find(|a| a.cpu == "cf" && a.kind == "code" && a.name.starts_with("cf."))
        .unwrap();
    (a.name.clone(), a.addr as u32)
}

#[test]
fn a_service_object_names_firmware_through_externs_and_cf_names() {
    let hook = "cf.page.hook.source_draw";
    let manifest = SERVICE.replace(
        "place = \"cf.expansion\"\n",
        &format!(
            "place = \"cf.expansion\"\n[object.externs]\n_x_exit = \"hook.{hook}.exit.2\"\n_x_tail = \"hook.{hook}.tail\"\n_x_fn = \"cf.draw.fn.blit\"\n"
        ),
    );
    let mut obj = service_object();
    obj.sections[1] = sec(".data", "data", 4, &[0; 48]);
    // `dkfw` links the firmware name itself; the aliases are the manifest's; dk_log stays an import of the runtime
    for (i, n) in [
        "_x_exit",
        "_x_tail",
        "_x_fn",
        "cf.input.fn.key_down",
        "dk_log",
        "_x_fn",
    ]
    .iter()
    .enumerate()
    {
        obj.relocs.push(rel(24 + 4 * i as u64, n));
    }
    obj.stamp_isa();
    let lib = Library {
        modules: vec![Module {
            manifest: toml::from_str(&manifest).unwrap(),
            objects: vec![obj],
            alloc: Default::default(),
            digest: String::new(),
            dir: Default::default(),
        }],
    };
    let o = PackOptions {
        device: Some("dt2".into()),
        ..Default::default()
    };
    let dkm = pack_module(&lib, "test/svc-test", &o).unwrap().dkm;
    let mut want = vec![
        patchkit::bind::exit_import(hook, 2),
        patchkit::bind::tail_import(hook),
        patchkit::bind::fw_import("cf.draw.fn.blit"),
        patchkit::bind::fw_import("cf.input.fn.key_down"),
        "dk_log".to_string(),
    ];
    want.sort();
    let mut got = dkm.imports.clone();
    got.sort();
    assert_eq!(got, want);
    // an extern that is no firmware reference is refused
    let bad = manifest.replace("cf.draw.fn.blit", "machines.first");
    let o = PackOptions {
        device: Some("dt2".into()),
        ..Default::default()
    };
    let lib = Library {
        modules: vec![Module {
            manifest: toml::from_str(&bad).unwrap(),
            objects: vec![{
                let mut x = service_object();
                x.sections[1] = sec(".data", "data", 4, &[0; 28]);
                x.relocs.push(rel(24, "_x_fn"));
                x.stamp_isa();
                x
            }],
            alloc: Default::default(),
            digest: String::new(),
            dir: Default::default(),
        }],
    };
    let e = pack_module(&lib, "test/svc-test", &o)
        .err()
        .unwrap()
        .to_string();
    assert!(e.contains("_x_fn"), "{e}");
}

#[test]
fn bind_resolves_the_firmware_references_of_a_raw_hook() {
    need_image!(DT2);
    let raw = image(DT2).unwrap();
    let reg = registry();
    let (hook, mut decls, site) = raw_hook(&raw, &reg);
    let (anchor, anchor_addr) = some_anchor(&raw, &reg);
    // the hook gets an exit 1: the anchor plus 4
    let key = format!("name = \"{hook}\"\n");
    let i = decls
        .iter()
        .position(|t| t.contains(&key) && t.contains("[[hook]]"))
        .unwrap();
    decls[i] = decls[i].replacen(&key, &format!("{key}exits = [\"{anchor} + 4\"]\n"), 1);
    let decls: Vec<&str> = decls.iter().map(String::as_str).collect();
    let imports = vec![
        patchkit::bind::fw_import(&anchor),
        patchkit::bind::exit_import(&hook, 0),
        patchkit::bind::exit_import(&hook, 1),
        patchkit::bind::stock_import(&hook),
        patchkit::bind::ret_import(&hook),
        patchkit::bind::tail_import(&hook),
    ];
    let dks = refs_service(Some(&hook), "dt2", &imports);
    let (file, hooks) = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap();
    let dkm = Dkm::parse(&file).unwrap();
    assert!(
        dkm.imports.iter().all(|n| !n.starts_with("dk_"))
            || dkm.imports.iter().all(|n| n == "dk_log")
    );
    let tail = hooks[0].tail_off.unwrap();
    // absolute: no relocation entry; base-relative: kind 0
    assert_eq!(word_at(&dkm, 24), anchor_addr);
    assert_eq!(word_at(&dkm, 28), site.ret as u32);
    assert_eq!(word_at(&dkm, 32), anchor_addr + 4);
    assert_eq!(
        word_at(&dkm, 36),
        tail,
        "dk_stock is the stock path of the hook"
    );
    assert_eq!(word_at(&dkm, 44), tail);
    let ret = word_at(&dkm, 40) as usize;
    assert_eq!(&dkm.image[ret..ret + 2], [0x4e, 0x75]);
    for at in [24usize, 28, 32] {
        let at = (data_base() + at) as u32;
        assert!(dkm.relocs.iter().all(|(o, _)| *o != at), "{at}");
    }
    for at in [36usize, 40, 44] {
        let at = (data_base() + at) as u32;
        assert!(
            dkm.relocs.iter().any(|(o, k)| *o == at && k.is_none()),
            "{at}"
        );
    }
    // the file carries the identity of the firmware: the first 8 bytes of its SHA-256
    let k = Keys::parse(&dkm.register).unwrap();
    let id = patchkit::error::unhex(&Firmware::parse(&raw).unwrap().syx_sha256).unwrap();
    assert_eq!((k.fw_count(), &k.fw(0).unwrap()[..]), (1, &id[..8]));
    // an exit the hook does not declare, and a name no firmware has, are errors naming them
    let dks = refs_service(Some(&hook), "dt2", &[patchkit::bind::exit_import(&hook, 2)]);
    let e = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap_err();
    assert!(e.0.contains("1 exit"), "{e}");
    let dks = refs_service(
        Some(&hook),
        "dt2",
        &[patchkit::bind::fw_import("cf.no.such.name")],
    );
    let e = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap_err();
    assert!(e.0.contains("cf.no.such.name") || e.0.contains('#'), "{e}");
}

#[test]
fn fw_is_written_only_for_a_file_with_a_firmware_reference_or_a_hook() {
    need_image!(DT2);
    let raw = image(DT2).unwrap();
    let reg = registry();
    let texts = patchkit::fwgen::fw_texts(&root()).unwrap();
    let decls: Vec<&str> = texts.iter().map(String::as_str).collect();
    let (anchor, addr) = some_anchor(&raw, &reg);
    // a reference and no hook: bound, with FW and no BIND
    let dks = refs_service(None, "dt2", &[patchkit::bind::fw_import(&anchor)]);
    let (file, hooks) = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap();
    let dkm = Dkm::parse(&file).unwrap();
    let k = Keys::parse(&dkm.register).unwrap();
    assert_eq!((hooks.len(), k.bind_count(), k.fw_count()), (0, 0, 1));
    assert_eq!(word_at(&dkm, 24), addr);
    // a hook (BIND) with no references: FW too
    let (file, _) = patchkit::bind::bind(&pack_service("dt2"), &raw, &reg, &decls).unwrap();
    let bound = Dkm::parse(&file).unwrap();
    let k = Keys::parse(&bound.register).unwrap();
    assert_eq!((k.bind_count(), k.fw_count()), (1, 1));
    // no hook and no reference: the file as built
    let dks = refs_service(None, "dt2", &[]);
    let (file, _) = patchkit::bind::bind(&dks, &raw, &reg, &decls).unwrap();
    assert_eq!(file, dks);
}

/// The hooks whose exit 0 the services use (`dk_exit_<hook>_0`: the end of the site) end where the stock code they replaced continued before bind resolved it: the anchor plus
/// the length the old image tables (`machines_info`, `fxpages_info` version 3) gave. The other exits are `exits` expressions of `devices/fw`.
#[test]
fn the_end_of_the_site_of_each_service_hook_is_where_the_stock_code_continues() {
    let both: [(&str, &str, u64); 9] = [
        ("cf.page.hook.page_desc", "cf.page.fn.page_desc", 6),
        (
            "cf.page.hook.fx_view_list.load",
            "cf.page.site.fx_view_list.load",
            6,
        ),
        (
            "cf.page.hook.fx_key_list.load",
            "cf.page.site.fx_key_list.load",
            6,
        ),
        (
            "cf.page.hook.sound_load_exit",
            "cf.page.site.sound_load_exit",
            8,
        ),
        (
            "cf.page.hook.tab_label_test",
            "cf.page.site.tab_label_test",
            6,
        ),
        (
            "cf.page.hook.filter_graph_test",
            "cf.page.site.filter_graph_test",
            6,
        ),
        (
            "cf.page.hook.fx_array.load",
            "cf.page.site.fx_array.load",
            6,
        ),
        (
            "cf.page.hook.fx_array.save",
            "cf.page.site.fx_array.save",
            6,
        ),
        (
            "cf.page.hook.fx_array.init_exit",
            "cf.page.site.fx_array.init_exit",
            8,
        ),
    ];
    let dn2_only: [(&str, &str, u64); 3] = [
        ("cf.machine.hook.group", "cf.machine.site.group", 6),
        (
            "cf.machine.hook.owner_type_load",
            "cf.machine.site.owner_type_load",
            6,
        ),
        ("cf.machine.hook.frametype", "cf.machine.site.frametype", 6),
    ];
    let reg = registry();
    for (img, extra) in [(DT2, &[][..]), (DN2, &dn2_only[..])] {
        need_image!(img);
        let raw = image(img).unwrap();
        let f = Firmware::parse(&raw).unwrap();
        let port = reg.find(&f.syx_sha256).unwrap();
        for (hook, anchor, len) in both.iter().chain(extra) {
            let site = patchkit::apply::resolve_hook_site(&raw, &reg, hook).unwrap();
            let at = port.anchor(anchor).unwrap().addr;
            assert_eq!(site.ret, at + len, "{img}: {hook}");
        }
    }
}
