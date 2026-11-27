//! The ISA check record of ColdFire objects (`patchkit build` writes it, `patchkit pack` verifies it).

use patchkit::dkm;
use patchkit::error::{hex, unhex};
use patchkit::module::Library;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn opts() -> dkm::Options {
    dkm::Options {
        name: "test/x".into(),
        version: 0x010203,
        trace: false,
        stereo: false,
        gate: false,
        service: None,
        payloads: vec![],
    }
}

/// A committed object packs; the same object with a byte of its code changed (no new record) is refused by `pack`.
#[test]
fn pack_refuses_a_tampered_object() {
    let Some(lib) = Library::scan_repo_or_skip(&root()) else {
        return;
    };
    let m = lib.get("test/dyn-crash").unwrap();
    let o = m.objects.iter().find(|o| o.arch == "cf").unwrap();
    assert!(dkm::pack(o, &opts()).is_ok());
    let mut t = o.clone();
    let s = t.sections.iter_mut().find(|s| s.kind == "text").unwrap();
    let mut b = unhex(&s.hex).unwrap();
    b[1] ^= 0x10;
    s.hex = hex(&b);
    let e = dkm::pack(&t, &opts()).unwrap_err().0;
    assert!(e.contains("ISA check record does not match"), "{e}");
    // without a record at all
    let mut u = o.clone();
    u.toolchain.remove(patchkit::dko::ISA_CHECK_KEY);
    assert!(
        dkm::pack(&u, &opts())
            .unwrap_err()
            .0
            .contains("no ISA check record")
    );
}
