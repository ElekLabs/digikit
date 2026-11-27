//! `patchkit::flash` against the stock images in `firmware/` (skipped when absent): prepare, restore, and tampered images.

use dt2_firmware_loader as loader;
use patchkit::firmware::Firmware;
use patchkit::flash::{self, Prepared};
use patchkit::module::Library;
use patchkit::port::Registry;
use patchkit::syx;
use patchkit::typed::Models;
use patchkit::verify;
use std::path::PathBuf;
use std::sync::OnceLock;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn reg() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| Registry::from_dir(&root().join("ports")).unwrap())
}

fn lib() -> &'static Library {
    static L: OnceLock<Library> = OnceLock::new();
    L.get_or_init(|| {
        // only `core/*` modules: their objects are committed, no content module is built
        let mut lib = Library::scan_repo_for(&root(), false, Some(&[])).unwrap();
        lib.lower(&Models::from_dir(&root().join("devices")).unwrap())
            .unwrap();
        lib
    })
}

fn stock(file: &str) -> Option<Vec<u8>> {
    std::fs::read(root().join("firmware").join(file)).ok()
}

const DT2: &str = "Digitakt_II_OS1.17.syx";
const DN2: &str = "Digitone_II_OS1.12.syx";

fn failed(p: &Prepared) -> String {
    p.failed()
        .iter()
        .map(|c| format!("{}: {}", c.name, c.detail))
        .collect::<Vec<_>>()
        .join(" | ")
}

fn names(p: &Prepared) -> Vec<&str> {
    p.failed().iter().map(|c| c.name.as_str()).collect()
}

fn patched_with_base() -> Option<&'static (Prepared, Vec<u8>)> {
    static P: OnceLock<Option<(Prepared, Vec<u8>)>> = OnceLock::new();
    P.get_or_init(|| {
        let s = stock(DT2)?;
        Some((flash::prepare(&s, reg(), lib(), "device").unwrap(), s))
    })
    .as_ref()
}

fn patched_dt2() -> Option<&'static Prepared> {
    patched_with_base().map(|p| &p.0)
}

/// The stock image the patched one was made from.
fn base_of_patched() -> Option<Vec<u8>> {
    patched_with_base().map(|p| p.1.clone())
}

/// The image's stream rebuilt with `f` applied (the framing messages stay).
fn restream(image: &[u8], f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let fw = Firmware::parse(image).unwrap();
    let mut s = loader::decode_syx(image).unwrap();
    f(&mut s);
    syx::encode_syx(&s, fw.device_id, &fw.framing_start, &fw.framing_end)
}

/// Make the content checksum of a stream match its container again.
fn fix_sum(s: &mut [u8]) {
    let total = u32::from_be_bytes(s[0..4].try_into().unwrap()) as usize;
    let sum = syx::content_checksum(&s[8..8 + total]);
    s[4..8].copy_from_slice(&sum.to_be_bytes());
}

#[test]
fn a_dt2_patch_is_prepared_with_every_check_and_an_image_id() {
    let Some(p) = patched_dt2() else { return };
    assert!(p.ok(), "{}", failed(p));
    let n: Vec<&str> = p.checks.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        n,
        [
            "stock",
            "patch",
            "preflight",
            "verify",
            "product",
            "build floor",
            "DSP scan",
            "round trip",
            "image id"
        ]
    );
    assert_eq!((p.device.as_str(), p.os.as_str()), ("dt2", "1.17"));
    assert_ne!(p.sha_image, p.sha_stock);
    assert!(p.image_id.is_some());
    // a header, the data messages the header announces, a footer
    let announced = (p.message(0)[12] as usize) << 14
        | (p.message(0)[13] as usize) << 7
        | p.message(0)[14] as usize;
    assert_eq!(p.messages.len(), announced + 2);
    assert!(announced > 14_703, "the patch grows the image");
    assert_eq!(p.message(0).len(), 16);
    assert_eq!(p.message(1).len(), 128);
    assert_eq!(p.message(p.messages.len() - 1).len(), 16);
    assert!(!p.restore);
}

#[test]
fn a_dn2_patch_is_prepared_and_its_id_differs_from_the_dt2_one() {
    let (Some(s), Some(dt2)) = (stock(DN2), patched_dt2()) else {
        return;
    };
    let p = flash::prepare(&s, reg(), lib(), "device").unwrap();
    assert!(p.ok(), "{}", failed(&p));
    assert_eq!((p.device.as_str(), p.os.as_str()), ("dn2", "1.12"));
    assert!(p.image_id.is_some());
    assert_ne!(p.image_id, dt2.image_id);
    let floor = p.checks.iter().find(|c| c.name == "build floor").unwrap();
    assert!(floor.detail.contains("no build floor"), "{}", floor.detail);
}

#[test]
fn the_image_id_is_the_same_on_every_run_and_changes_with_the_patch() {
    let (Some(s), Some(p)) = (stock(DT2), patched_dt2()) else {
        return;
    };
    let again = flash::prepare(&s, reg(), lib(), "device").unwrap();
    assert_eq!(again.image_id, p.image_id);
    assert_eq!(again.sha_image, p.sha_image);
    // another patch set is another image when its bytes differ
    let emu = flash::prepare(&s, reg(), lib(), "emulator").unwrap();
    // the two sets are equal today; the id must follow the bytes, whichever it is
    assert_eq!(emu.image_id == p.image_id, emu.sha_image == p.sha_image);
}

#[test]
fn a_stock_file_is_restored_through_the_same_checks() {
    for f in [DT2, DN2] {
        let Some(s) = stock(f) else { continue };
        let r = flash::restore(&s, reg()).unwrap();
        assert!(r.ok(), "{}", failed(&r));
        assert!(r.restore);
        assert_eq!(r.image, s);
        assert_eq!(r.image_id, None);
        let n: Vec<&str> = r.checks.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            n,
            [
                "stock",
                "verify",
                "product",
                "build floor",
                "DSP scan",
                "round trip"
            ]
        );
    }
}

#[test]
fn a_patched_or_unknown_file_is_not_a_stock_file() {
    let (Some(s), Some(p)) = (stock(DT2), patched_dt2()) else {
        return;
    };
    let e = flash::restore(&p.image, reg()).unwrap_err().0;
    assert!(e.contains("patched image"), "{e}");
    let e = flash::prepare(&p.image, reg(), lib(), "device")
        .unwrap_err()
        .0;
    assert!(e.contains("patched image"), "{e}");
    // a stock file with one changed byte is no known firmware
    let odd = restream(&s, |s| s[0x200] ^= 1);
    let e = flash::restore(&odd, reg()).unwrap_err().0;
    assert!(e.contains("not a stock firmware"), "{e}");
    assert!(flash::prepare(&odd, reg(), lib(), "device").is_err());
    assert!(flash::prepare(&s, reg(), lib(), "nope").is_err());
}

#[test]
fn a_flipped_data_byte_fails_verify_and_the_round_trip() {
    let Some(p) = patched_dt2() else { return };
    let mut bad = p.image.clone();
    bad[16 + 128 * 50 + 40] ^= 1;
    let c = flash::check_image(bad, reg(), base_of_patched().as_deref()).unwrap();
    assert!(!c.ok());
    let f = names(&c);
    assert!(f.contains(&"verify") && f.contains(&"round trip"), "{f:?}");
}

#[test]
fn a_flipped_byte_with_the_checksums_fixed_fails_the_hmac() {
    let Some(p) = patched_dt2() else { return };
    let bad = restream(&p.image, |s| {
        s[8 + 0x400] ^= 0x10;
        fix_sum(s);
    });
    let c = flash::check_image(bad, reg(), base_of_patched().as_deref()).unwrap();
    assert!(!c.ok());
    let verify = c.checks.iter().find(|c| c.name == "verify").unwrap();
    assert!(verify.detail.contains("HMAC"), "{}", verify.detail);
    let rt = c.checks.iter().find(|c| c.name == "round trip").unwrap();
    assert!(rt.detail.contains("HMAC"), "{}", rt.detail);
}

#[test]
fn a_wrong_product_byte_fails() {
    let Some(p) = patched_dt2() else { return };
    let mut bad = p.image.clone();
    let end = bad.len();
    bad[8] = 0x10;
    bad[end - 8] = 0x10;
    let c = flash::check_image(bad, reg(), base_of_patched().as_deref()).unwrap();
    assert!(names(&c).contains(&"product"), "{}", failed(&c));
    // a DN2 stream with a DT2 header byte
    let mut bad = p.image.clone();
    bad[4] = 0x15;
    assert!(
        flash::check_image(bad, reg(), base_of_patched().as_deref())
            .map(|c| !c.ok())
            .unwrap_or(true)
    );
}

#[test]
fn a_dt2_build_word_below_the_floor_fails() {
    let Some(p) = patched_dt2() else { return };
    let bad = restream(&p.image, |s| {
        s[flash::BUILD_WORD_AT..flash::BUILD_WORD_AT + 4].copy_from_slice(b"0059");
        fix_sum(s);
    });
    let c = flash::check_image(bad, reg(), base_of_patched().as_deref()).unwrap();
    let f = names(&c);
    assert!(f.contains(&"build floor"), "{f:?}");
    let rt = c.checks.iter().find(|c| c.name == "round trip").unwrap();
    assert!(rt.detail.contains("floor"), "{}", rt.detail);
}

#[test]
fn an_oversize_container_fails_the_limits() {
    let Some(p) = patched_dt2() else { return };
    let mut fw = Firmware::parse(&p.image).unwrap();
    let mut meta = fw.section(5).unwrap().data.clone();
    // incompressible bytes in a section that is stored as it is, so that the container grows past the container limit
    let mut x: u32 = 1;
    meta.extend((0..1_700_000).map(|_| {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (x >> 24) as u8
    }));
    fw.replace(5, meta).unwrap();
    let big = fw.build(None).unwrap();
    let c = flash::check_image(big, reg(), base_of_patched().as_deref()).unwrap();
    let v = c.checks.iter().find(|c| c.name == "verify").unwrap();
    assert!(!v.ok && v.detail.contains("limit"), "{}", v.detail);
    assert!(!c.ok());
}

#[test]
fn the_build_cap_is_the_container_length() {
    let Some(p) = patched_dt2() else { return };
    let fw = Firmware::parse(&p.image).unwrap();
    // the refusal names the container length; a cap of exactly that length builds, one byte less does not
    let e = fw.build(Some(1)).unwrap_err().to_string();
    let len: usize = e.split_whitespace().nth(2).unwrap().parse().unwrap(); // "container is N bytes, ..."
    assert!(fw.build(Some(len)).is_ok());
    assert!(fw.build(Some(len - 1)).is_err());
}

#[test]
fn a_message_list_with_one_message_dropped_reordered_or_altered_fails() {
    let Some(p) = patched_dt2() else { return };
    let msgs: Vec<Vec<u8>> = (0..p.messages.len())
        .map(|i| p.message(i).to_vec())
        .collect();
    let check = |m: &[Vec<u8>]| {
        let c =
            flash::check_image(m.concat(), reg(), base_of_patched().as_deref()).map(|c| c.checks);
        match c {
            Ok(cs) => cs.iter().any(|c| c.name == "round trip" && !c.ok),
            Err(_) => true,
        }
    };
    let mut dropped = msgs.clone();
    dropped.remove(7000);
    assert!(check(&dropped));
    let mut swapped = msgs.clone();
    swapped.swap(3000, 3001);
    assert!(check(&swapped));
    let mut altered = msgs.clone();
    altered[9000][50] ^= 0x01;
    assert!(check(&altered));
    assert!(!check(&msgs));
}

#[test]
fn a_container_read_back_from_flash_makes_the_image_again() {
    for f in [DT2, DN2] {
        let Some(s) = stock(f) else { continue };
        let container = flash::container_of(&s).unwrap();
        // flash holds erased bytes after the container
        let mut in_flash = container.clone();
        in_flash.extend(std::iter::repeat_n(0xff, 0x10000));
        assert_eq!(flash::image_from_flash(&in_flash, &s).unwrap(), s);
        // a patched image too, built from another image's framing
        let mut bad = in_flash.clone();
        bad[0x400] ^= 1;
        assert!(
            flash::image_from_flash(&bad, &s)
                .unwrap_err()
                .0
                .contains("HMAC")
        );
        assert!(flash::image_from_flash(&[0xff; 64], &s).is_err());
    }
    let Some(p) = patched_dt2() else { return };
    let mut in_flash = flash::container_of(&p.image).unwrap();
    in_flash.extend(std::iter::repeat_n(0xff, 0x4000));
    assert_eq!(
        flash::image_from_flash(&in_flash, &p.image).unwrap(),
        p.image
    );
}

/// A same-DAG store (`DM(-16, I6) = I0`) after ten NOPs, as boot-stream bytes: the ISA does not have it, the silicon does not run it.
fn bad_store_code() -> Vec<u8> {
    let nops = "    NOP;\n".repeat(10);
    // the assembler refuses the store itself: its ISA word (Selache order) between the NOPs and the RTS
    let mut code = patchkit::build::sharc_obj::boot_bytes_unchecked_for_tests(&format!(
        ".SECTION/PM seg_pmco;\n_x:\n{nops}"
    ))
    .unwrap();
    code.extend([0x08, 0x9d, 0x70, 0x08]);
    code.extend(
        patchkit::build::sharc_obj::boot_bytes_unchecked_for_tests(
            ".SECTION/PM seg_pmco;\n    RTS;\n",
        )
        .unwrap(),
    );
    code
}

#[test]
fn a_dsp_section_with_a_store_the_isa_does_not_have_is_refused() {
    for f in [DT2, DN2] {
        let Some(s) = stock(f) else { continue };
        let clean = flash::check_image(s.clone(), reg(), Some(&s)).unwrap();
        let scan = clean.checks.iter().find(|c| c.name == "DSP scan").unwrap();
        assert!(scan.ok, "{f}: {}", scan.detail);
        let mut fw = Firmware::parse(&s).unwrap();
        let mut dsp = patchkit::dsp::Stream::parse(fw.section(7).unwrap().data.clone()).unwrap();
        // the first free address
        let code = bad_store_code();
        let mut last = None;
        for a in (0x2001_0000..patchkit::dsp::L2_BOOT_LIMIT).step_by(0x1000) {
            match dsp.append_block(a, &code) {
                Ok(()) => {
                    last = None;
                    break;
                }
                Err(e) => last = Some(e),
            }
        }
        if let Some(e) = last {
            panic!("no free address in L2: {e}");
        }
        fw.replace(7, dsp.blob).unwrap();
        let p = flash::check_image(fw.build(None).unwrap(), reg(), Some(&s)).unwrap();
        assert!(!p.ok());
        let c = p.checks.iter().find(|c| c.name == "DSP scan").unwrap();
        assert!(
            !c.ok && c.detail.contains("DM store of I0 (DAG1)"),
            "{f}: {}",
            c.detail
        );
        assert!(p.messages.is_empty() || !p.ok());
    }
}

#[test]
fn the_summary_tiles_the_image_and_a_checkout_prepare_makes_the_same_image() {
    let (Some(p), Some(s)) = (patched_dt2(), stock(DT2)) else {
        return;
    };
    let j = p.summary_json();
    assert_eq!(j["ok"], true);
    assert_eq!(j["restore"], false);
    assert_eq!(j["image_id"], p.image_id_hex().unwrap());
    assert_eq!(
        j["runtime"],
        patchkit::dkm::fmt_runtime(patchkit::dkm::runtime_version())
    );
    // the message ends are the message ranges: a header, 128-byte data messages, a footer
    let ends: Vec<usize> = j["ends"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_u64().unwrap() as usize)
        .collect();
    assert_eq!(ends, p.messages.iter().map(|m| m.end).collect::<Vec<_>>());
    assert_eq!((ends[0], *ends.last().unwrap()), (16, p.image.len()));
    // the desktop app's entry reads the checkout and gives the image `prepare` gives
    let r = flash::prepare_in_repo(&root(), &s, false, "device").unwrap();
    assert_eq!(r.image, p.image);
    assert_eq!(r.image_id, p.image_id);
    let back = flash::prepare_in_repo(&root(), &s, true, "device").unwrap();
    assert!(back.restore && back.ok());
    assert_eq!(back.image, s);
    assert_eq!(back.summary_json()["image_id"], serde_json::Value::Null);
}

/// The stock images in `firmware/` that a port knows (any device), as (file name, bytes).
fn known_stock_images() -> Vec<(String, Vec<u8>)> {
    let mut out = vec![];
    for e in std::fs::read_dir(root().join("firmware"))
        .unwrap()
        .flatten()
    {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(bytes) = std::fs::read(e.path()) else {
            continue;
        };
        if name.ends_with(".syx")
            && Firmware::parse(&bytes).is_ok_and(|fw| reg().find(&fw.syx_sha256).is_some())
        {
            out.push((name, bytes));
        }
    }
    out
}

/// An image whose read-only sections are the stock ones but whose hash is not a stock hash.
fn changed_meta(stock: &[u8]) -> Vec<u8> {
    let mut fw = Firmware::parse(stock).unwrap();
    let mut meta = fw.section(5).unwrap().data.clone();
    meta.push(0);
    fw.replace(5, meta).unwrap();
    fw.build(None).unwrap()
}

/// The stock image with one byte of the stored form of section `id` flipped and the container sealed again.
fn damaged_section(stock: &[u8], id: u32) -> Vec<u8> {
    let mut fw = Firmware::parse(stock).unwrap();
    let sec = fw.sections.iter_mut().find(|s| s.id == id).unwrap();
    let n = sec.stored.len() - 2;
    sec.stored[n] ^= 0x01;
    fw.build(None).unwrap()
}

#[test]
fn verify_needs_the_stock_base_of_a_device_with_read_only_sections() {
    for (f, s) in known_stock_images() {
        let ro = Firmware::parse(&s)
            .unwrap()
            .device
            .readonly_sections()
            .to_vec();
        assert!(
            !ro.is_empty(),
            "{f}: the device file lists read-only sections"
        );
        // a stock image is its own base: its hash is a known stock hash
        verify::verify(&s, Some(reg()), None).unwrap_or_else(|e| panic!("{f}: {}", e.0));
        // an image of unknown hash, read-only sections untouched: no base fails, the base passes
        let changed = changed_meta(&s);
        let e = verify::verify(&changed, Some(reg()), None).unwrap_err().0;
        assert!(e.contains("stock base image is required"), "{f}: {e}");
        let e = verify::verify(&changed, None, None).unwrap_err().0;
        assert!(e.contains("stock base image is required"), "{f}: {e}");
        let ok = verify::verify(&changed, Some(reg()), Some(&s))
            .unwrap_or_else(|e| panic!("{f}: {}", e.0));
        assert!(
            ok.checks.iter().any(|c| c.contains("read-only sections")),
            "{f}: {:?}",
            ok.checks
        );
        // `inspect` identifies a file without a base and is no proof
        verify::inspect(&changed, Some(reg())).unwrap();
        // a damaged read-only section never passes, with or without the base; where the image still decodes the comparison names it
        let mut named = 0;
        for id in ro {
            if Firmware::parse(&s)
                .unwrap()
                .section(id)
                .is_none_or(|x| x.stored.len() < 4)
            {
                continue;
            }
            let bad = damaged_section(&s, id);
            assert!(
                verify::verify(&bad, Some(reg()), None).is_err(),
                "{f}: section {id} without base"
            );
            let e = verify::verify(&bad, Some(reg()), Some(&s))
                .expect_err(&format!("{f}: section {id} with base passed"))
                .0;
            if e.contains(&format!("section {id} differs from the stock image")) {
                named += 1;
            }
        }
        assert!(
            named > 0,
            "{f}: no damaged read-only section was caught by the base comparison"
        );
    }
}
