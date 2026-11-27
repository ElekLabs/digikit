//! Service files: a module with `role = "service"` packs to a `.dks` with header role 1 and a version 2 header table with `PROVIDES` and `EXPORTS`; any
//! other module is a `.dkm` with role 0 and a version 1 table. The packing tests skip when the m68k-elf toolchain is not installed.

use dkneeds::keys::{Keys, name_hash};
use patchkit::build::toolchain::Env;
use patchkit::dkm::Dkm;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &p);
        } else {
            std::fs::copy(e.path(), p).unwrap();
        }
    }
}

/// A repository root of its own for a refusal that happens when the manifest loads: the repository's module roots, `ports`, `devices` and `sdk`, and the fixture
/// `test/svc-test` with the text of its `module.toml` changed by `edit`. No toolchain is needed.
fn manifest_root(dir: &Path, edit: &dyn Fn(&str) -> String) {
    use std::os::unix::fs::symlink;
    std::fs::create_dir_all(dir).unwrap();
    for n in ["modules", "runtime", "services", "ports", "devices", "sdk"] {
        symlink(root().join(n), dir.join(n)).unwrap();
    }
    let to = dir.join("tests/fixtures/modules/svc-test");
    copy_dir(&root().join("tests/fixtures/modules/svc-test"), &to);
    let text = std::fs::read_to_string(to.join("module.toml")).unwrap();
    std::fs::write(to.join("module.toml"), edit(&text)).unwrap();
}

/// `patchkit --root ROOT pack ID --device dt2 -o OUT`.
fn pack(root: &Path, id: &str, out: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_patchkit"))
        .arg("--root")
        .arg(root)
        .args(["pack", id, "--device", "dt2", "-o"])
        .arg(out)
        .output()
        .unwrap()
}

fn packed(root: &Path, id: &str, out: &Path) -> Vec<u8> {
    let r = pack(root, id, out);
    assert!(
        r.status.success(),
        "pack {id} failed:\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    std::fs::read(out).unwrap()
}

fn refused(root: &Path, id: &str, out: &Path) -> String {
    let r = pack(root, id, out);
    assert!(!r.status.success(), "pack {id} must be refused");
    String::from_utf8_lossy(&r.stderr).into_owned()
}

fn toolchain() -> bool {
    Env::new(&root()).m68k_or_skip()
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("pk-svc-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_service_packs_to_a_dks_with_provides_and_exports_and_a_user_has_a_hard_need() {
    if !toolchain() {
        return;
    }
    let here = root();
    let d = scratch("pack");

    let svc = packed(&here, "test/svc-test", &d.join("svc-test.dks"));
    let dkm = Dkm::parse(&svc).unwrap();
    assert_eq!(dkm.role, 1);
    assert_eq!(&svc[0x38..0x3c], &1u32.to_be_bytes());
    assert_eq!(&dkm.register[4..6], &2u16.to_be_bytes(), "table version 2");
    let k = Keys::parse(&dkm.register).unwrap();
    let provides: Vec<_> = k.provides().collect();
    assert_eq!(provides, vec![(name_hash("svc-test"), dkm.version)]);
    let exports: Vec<_> = k.exports().collect();
    let mut names: Vec<u32> = exports.iter().map(|e| e.a).collect();
    names.sort();
    let mut want = vec![name_hash("dk_svctest_ping"), name_hash("dk_svctest_hits")];
    want.sort();
    assert_eq!(names, want);
    assert!(exports.iter().all(|e| (e.b as usize) < dkm.image.len()));
    let hooks: Vec<_> = k.hooks().collect();
    assert_eq!(hooks.len(), 1, "the hook of the manifest");
    assert_eq!(hooks[0].a, name_hash("cf.menu.hook.settings_list.test"));
    assert!(((hooks[0].b & dkneeds::keys::HOOK_ENTRY_MASK) as usize) < dkm.image.len());
    // a service that takes a hook (or names the firmware) has a hard need of the port file
    let port = k
        .needs()
        .find(|n| n.service == name_hash("port"))
        .expect("a need of port");
    assert!(port.hard);

    let user = packed(&here, "test/svc-test-user", &d.join("svc-test-user.dkm"));
    let dkm = Dkm::parse(&user).unwrap();
    assert_eq!(dkm.role, 0);
    assert_eq!(&user[0x38..0x3c], &[0u8; 4]);
    assert_eq!(&dkm.register[4..6], &1u16.to_be_bytes(), "table version 1");
    let k = Keys::parse(&dkm.register).unwrap();
    let needs: Vec<_> = k.needs().collect();
    // the service of the test imports, and the runtime's own service (dk_log of `init`)
    let need = needs
        .iter()
        .find(|n| n.service == name_hash("svc-test"))
        .expect("a need of svc-test");
    assert!(need.hard);
    assert!(
        needs
            .iter()
            .all(|n| n.service == name_hash("svc-test") || n.service == name_hash("runtime"))
    );
    assert_eq!(k.provides().count(), 0);

    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_service_with_a_reserved_service_or_a_foreign_export_is_refused() {
    let d = scratch("bad");
    let cases: [(&str, &dyn Fn(&str) -> String, &str); 8] = [
        (
            "reserved service",
            &|t| t.replace("service = \"svc-test\"", "service = \"port\""),
            "unreserved service name",
        ),
        (
            "no service",
            &|t| t.replace("service = \"svc-test\"\n", ""),
            "needs `service",
        ),
        (
            "export in the manifest",
            &|t| format!("{t}\n[[export]]\nname = \"dk_svctest_ping\"\n"),
            "[[export]] \"dk_svctest_ping\" is not read",
        ),
        (
            "id that is not the service name",
            &|t| t.replace("id = \"test/svc-test\"", "id = \"test/other-name\""),
            "must equal the provided service",
        ),
        (
            "id leaf over 50 bytes",
            &|t| {
                t.replace(
                    "id = \"test/svc-test\"",
                    &format!("id = \"test/{}\"", "x".repeat(51)),
                )
            },
            "at most 50",
        ),
        (
            "DSP fill of a slot that is not in [dsp.slots]",
            &|t| {
                format!("{t}\n[[fill]]\nslot = \"no_such_slot\"\nentry = \"_x\"\nspace = \"dsp\"\n")
            },
            "has no such slot",
        ),
        (
            "fill in another space",
            &|t| {
                format!("{t}\n[[fill]]\nslot = \"voice_render\"\nentry = \"_x\"\nspace = \"pru\"\n")
            },
            "is not \"cf\" or \"dsp\"",
        ),
        (
            "service without the role",
            &|t| t.replace("role = \"service\"\n", ""),
            "belong to a module with `role = \"service\"`",
        ),
    ];
    for (i, (what, edit, want)) in cases.iter().enumerate() {
        let r = d.join(format!("root{i}"));
        manifest_root(&r, *edit);
        let e = refused(&r, "test/svc-test", &d.join("x.dks"));
        assert!(e.contains(want), "{what}: {e}");
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn local_service_packages_declare_the_export_order_and_a_bound_versioned_need() {
    if !toolchain() {
        return;
    }
    use dkneeds::keys::tag;
    let d = scratch("local");
    let service = name_hash("local-ping");
    let symbol = name_hash("local_ping_value");
    let provider = packed(&root(), "test/local-ping", &d.join("local-ping.dks"));
    let provider = Dkm::parse(&provider).unwrap();
    let keys = Keys::parse(&provider.register).unwrap();
    assert_eq!(keys.provides().collect::<Vec<_>>(), [(service, 0x010200)]);
    // the exported function, then the resident query of the Query part
    let query = name_hash(dkrt_sdk::RESIDENT_QUERY_ENTRY);
    assert_eq!(
        keys.exports().map(|e| e.a).collect::<Vec<_>>(),
        [symbol, query]
    );
    assert_eq!(keys.words(tag::CALLBACK_ORDER, 0), Some((service, 450, 0)));
    assert!(
        keys.exports()
            .filter(|e| e.a == symbol)
            .all(|e| e.b & dkneeds::keys::EXPORT_BOUND != 0)
    );
    let floor = patchkit::sdkgen::service_registration_runtime();
    // the resident query of the Query part raises the runtime the provider needs above the floor of a service
    assert!(
        keys.needs()
            .any(|n| n.service == name_hash("runtime") && n.hard && n.min_version >= floor)
    );

    let user = packed(
        &root(),
        "test/local-ping-user",
        &d.join("local-ping-user.dkm"),
    );
    let user = Dkm::parse(&user).unwrap();
    let keys = Keys::parse(&user.register).unwrap();
    let need = keys.needs().find(|need| need.service == service).unwrap();
    assert!(need.hard);
    assert_eq!(need.min_version, 0x010200);
    assert_eq!(
        keys.words(tag::SERVICE_IMPORTS, 0),
        Some((symbol, service, 0))
    );
    assert!(
        keys.needs()
            .any(|n| n.service == name_hash("runtime") && n.hard && n.min_version == floor)
    );
    let _ = std::fs::remove_dir_all(d);
}

/// The big-endian bytes of `word` occur in a section of the committed object `path` (a `.dko.json`).
fn object_holds_word(path: &Path, word: u32) -> bool {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let want: String = word
        .to_be_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    v["sections"]
        .as_array()
        .unwrap()
        .iter()
        .any(|s| s["hex"].as_str().is_some_and(|h| h.contains(&want)))
}

#[test]
fn lfo_packs_to_a_dks_that_owns_lfo_shape_and_its_users_need_it() {
    if !toolchain() {
        return;
    }
    let here = root();
    let d = scratch("lfo");
    let svc = packed(&here, "core/services/lfo", &d.join("lfo.dks"));
    let dkm = Dkm::parse(&svc).unwrap();
    assert_eq!(dkm.role, 1);
    assert_eq!(&dkm.register[4..6], &2u16.to_be_bytes(), "table version 2");
    let k = Keys::parse(&dkm.register).unwrap();
    assert_eq!(
        k.provides().collect::<Vec<_>>(),
        vec![(name_hash("lfo"), dkm.version)]
    );
    // the kind and its handler (the glue declares init as the handler of every kind)
    assert!(k.kinds().eq([dkneeds::keys::KindDecl {
        kind: dkneeds::KIND_LFO_SHAPE,
        len: dkneeds::LEN_LFO_SHAPE,
        entry: dkm.init.unwrap(),
    }]));
    // the ABI word: set, and the value the runtime object carries (`dkrt_abi::SVC_ABI` of the ColdFire build, which the INFO reply reports)
    let abi = u32::from_be_bytes(svc[dkneeds::H_SVC_ABI as usize..][..4].try_into().unwrap());
    assert_ne!(abi, 0);
    assert!(
        object_holds_word(&here.join("runtime/core/build/runtime.dko.json"), abi),
        "H_SVC_ABI {abi:#x} is not the runtime's SVC_ABI: rebuild the objects (`patchkit build core/runtime`)"
    );

    // a module with an lfo_shape key needs the service; any other module of the tree needs no more than before
    let user = packed(&here, "test/lfo-shape", &d.join("lfo-shape.dkm"));
    let user = Dkm::parse(&user).unwrap();
    let k = Keys::parse(&user.register).unwrap();
    let lfo: Vec<_> = k
        .needs()
        .filter(|n| n.service == name_hash("lfo"))
        .collect();
    assert_eq!(lfo.len(), 1);
    assert!(lfo[0].hard);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn fx_packs_to_a_dks_that_owns_effect_and_takes_nine_hooks_by_entry() {
    if !toolchain() {
        return;
    }
    let here = root();
    let d = scratch("fx");
    let svc = packed(&here, "core/services/fx", &d.join("fx.dks"));
    let dkm = Dkm::parse(&svc).unwrap();
    assert_eq!(dkm.role, 1);
    let k = Keys::parse(&dkm.register).unwrap();
    assert_eq!(
        k.provides().collect::<Vec<_>>(),
        vec![(name_hash("fx"), dkm.version)]
    );
    assert!(k.kinds().eq([dkneeds::keys::KindDecl {
        kind: dkneeds::KIND_EFFECT,
        len: dkneeds::LEN_EFFECT,
        entry: dkm.init.unwrap(),
    }]));
    // the nine hooks of the effect pages, each by the offset of its entry in the image (the port file of the firmware lists the sites before the service is loaded)
    let hooks: Vec<_> = k.hooks().collect();
    assert_eq!(hooks.len(), 9);
    for name in [
        "cf.page.hook.page_desc",
        "cf.page.hook.tab_label_test",
        "cf.page.hook.filter_graph_test",
        "cf.page.hook.fx_view_list.load",
        "cf.page.hook.fx_array.load",
        "cf.page.hook.fx_array.save",
        "cf.page.hook.fx_array.init_exit",
        "cf.page.hook.fx_key_list.load",
        "cf.page.hook.sound_load_exit",
    ] {
        let h = hooks
            .iter()
            .find(|p| p.a == name_hash(name))
            .unwrap_or_else(|| panic!("no HOOKS entry for {name}"));
        assert!(
            (h.b as usize) < dkm.image.len(),
            "{name}: the entry is in the image"
        );
    }
    let abi = u32::from_be_bytes(svc[dkneeds::H_SVC_ABI as usize..][..4].try_into().unwrap());
    assert!(
        object_holds_word(&here.join("runtime/core/build/runtime.dko.json"), abi),
        "H_SVC_ABI {abi:#x} is not the runtime's SVC_ABI: rebuild the objects (`patchkit build core/runtime`)"
    );
    // an effect module needs the service: its effect record is a kind of the fx service, which has no row in the runtime any more
    let user = packed(&here, "test/dyn-fx-gain", &d.join("dyn-fx-gain.dkm"));
    let user = Dkm::parse(&user).unwrap();
    let k = Keys::parse(&user.register).unwrap();
    let needs: Vec<_> = k.needs().filter(|n| n.service == name_hash("fx")).collect();
    assert_eq!(needs.len(), 1);
    assert!(needs[0].hard);
    let _ = std::fs::remove_dir_all(&d);
}

/// The DSP part of the fixture (`src/dsp/svc_test_dsp.s`, no part of the manifest of `test/svc-test`) added as an object for the DT2 and a DSP fill of the slot `voice_unpack`.
fn with_dsp_part(t: &str) -> String {
    format!(
        "{t}\n[[object]]\npath = \"build/svc-test-dsp.dko.json\"\nsources = [\"src/dsp/svc_test_dsp.s\"]\nplace = \"dsp.l2\"\ndevices = [\"dt2\"]\n\n[[fill]]\nslot = \"voice_unpack\"\nentry = \"_svctest_dsp_entry\"\nspace = \"dsp\"\ndevices = [\"dt2\"]\n"
    )
}

/// `manifest_root` with the toolchains of the repository (`out/tools`), for a test that builds the fixture's objects in the scratch root.
fn building_root(dir: &Path, edit: &dyn Fn(&str) -> String) {
    manifest_root(dir, edit);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::os::unix::fs::symlink(root().join("out/tools"), dir.join("out/tools")).unwrap();
}

/// `patchkit --root ROOT pack ID --device DEVICE -o OUT`.
fn pack_for(root: &Path, id: &str, device: &str, out: &Path) -> Vec<u8> {
    let r = Command::new(env!("CARGO_BIN_EXE_patchkit"))
        .arg("--root")
        .arg(root)
        .args(["pack", id, "--device", device, "-o"])
        .arg(out)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "pack {id} --device {device} failed:\n{}",
        String::from_utf8_lossy(&r.stderr)
    );
    std::fs::read(out).unwrap()
}

#[test]
fn a_service_with_dsp_code_packs_a_dsp_section_and_a_dsp_slot_fill_for_its_device() {
    if !toolchain() {
        return;
    }
    let d = scratch("dsp");
    let r = d.join("root");
    building_root(&r, &with_dsp_part);
    let dkm = Dkm::parse(&pack_for(
        &r,
        "test/svc-test",
        "dt2",
        &d.join("svc-test.dks"),
    ))
    .unwrap();
    // the section is image, relocations and imports of the one DSP object: no voices, no effects
    let dsp = patchkit::dkmdsp::DspSection::parse(&dkm.dsp).unwrap();
    assert_eq!(dsp.imports, ["_svctest_dsp_back", "_svctest_dsp_word"]);
    assert_eq!((dsp.voices.len(), dsp.fxs.len()), (0, 0));
    assert_eq!(dsp.relocs.len(), 3, "the data word twice and the jump");
    // the SLOTS item names the slot by hash, carries the DSP flag and the byte offset of the entry in the DSP image: behind the first instruction
    let keys = Keys::parse(&dkm.register).unwrap();
    let slots: Vec<_> = keys.slots().collect();
    assert_eq!(slots.len(), 1);
    assert_eq!(slots[0].id, name_hash("voice_unpack"));
    assert_eq!(slots[0].flags, dkneeds::keys::SLOT_DSP);
    assert!(
        slots[0].entry > 0 && (slots[0].entry as usize) < dsp.image.len(),
        "entry {} in an image of {} bytes",
        slots[0].entry,
        dsp.image.len()
    );
    // the other device has no DSP object and no fill of its own: a file without DSP section and without the item
    let dn2 = Dkm::parse(&pack_for(
        &r,
        "test/svc-test",
        "dn2",
        &d.join("svc-test-dn2.dks"),
    ))
    .unwrap();
    assert!(dn2.dsp.is_empty());
    assert_eq!(Keys::parse(&dn2.register).unwrap().slots().count(), 0);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_dsp_fill_without_a_dsp_object_for_the_device_is_refused() {
    if !toolchain() {
        return;
    }
    let d = scratch("dspnone");
    let r = d.join("root");
    // the fill is there for every device, the DSP object only for the DT2
    building_root(&r, &|t| {
        with_dsp_part(t).replace(
            "entry = \"_svctest_dsp_entry\"\nspace = \"dsp\"\ndevices = [\"dt2\"]",
            "entry = \"_svctest_dsp_entry\"\nspace = \"dsp\"",
        )
    });
    let out = Command::new(env!("CARGO_BIN_EXE_patchkit"))
        .arg("--root")
        .arg(&r)
        .args(["pack", "test/svc-test", "--device", "dn2", "-o"])
        .arg(d.join("y.dks"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let e = String::from_utf8_lossy(&out.stderr);
    assert!(e.contains("no DSP section that defines it"), "{e}");
    let _ = std::fs::remove_dir_all(&d);
}

/// A service file is one file for every device: the fill of a slot that only one device has stays in it under a device mask in the flags of its `SLOTS` item
/// (the loader skips the other device's fills), so the packs for the two devices are the same bytes.
#[test]
fn fills_of_one_device_each_pack_to_the_same_file_under_a_device_mask() {
    if !toolchain() {
        return;
    }
    // the two runtime devices with a SHARC DSP, from the device registry
    let devs: Vec<_> = patchkit::devdata::all()
        .iter()
        .filter(|d| d.digikit_id.is_some() && d.identity.capabilities.dsp == "sharc")
        .take(2)
        .collect();
    assert_eq!(devs.len(), 2, "two runtime devices with a DSP");
    let (first, second) = (&devs[0].short, &devs[1].short);
    let d = scratch("fills-masked");
    let r = d.join("root");
    building_root(&r, &|t| {
        format!(
            "{t}\n[[fill]]\nslot = \"voice_unpack\"\nentry = \"svc_test_hook\"\ndevices = [\"{first}\"]\n\n[[fill]]\nslot = \"frametype\"\nentry = \"svc_test_hook\"\ndevices = [\"{second}\"]\n"
        )
    });
    let packs: Vec<Vec<u8>> = [first, second]
        .iter()
        .map(|dev| {
            pack_for(
                &r,
                "test/svc-test",
                dev,
                &d.join(format!("svc-test-{dev}.dks")),
            )
        })
        .collect();
    assert!(packs[0] == packs[1], "the packs differ between the devices");
    let keys_bytes = Dkm::parse(&packs[0]).unwrap().register;
    let slots: Vec<_> = Keys::parse(&keys_bytes).unwrap().slots().collect();
    assert_eq!(slots.len(), 2);
    let mut masks: Vec<u32> = slots
        .iter()
        .map(|s| (s.flags & dkneeds::keys::SLOT_DEVICES_MASK) >> dkneeds::keys::SLOT_DEVICES_SHIFT)
        .collect();
    masks.sort();
    let mut want: Vec<u32> = devs
        .iter()
        .map(|d| 1 << (d.digikit_id.unwrap() - 1))
        .collect();
    want.sort();
    assert_eq!(masks, want, "one fill for each device");
    let _ = std::fs::remove_dir_all(&d);
}
