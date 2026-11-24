use super::*;
use dkrt_sdk::E_NOT_FOUND;
use dkrt_test_support::{FakeCore, FakeHost};

const H: [u8; 32] = [7; 32];

fn sample_files(f: &FakeHost) {
    f.storage.add_file(
        SAMPLES,
        b"KICKS/K1",
        &[3u8; 100],
        dkrt_abi::storage::HASH_ELEKTRON32,
        H,
    );
    f.storage.add_file(
        SAMPLES,
        b"KICKS/K2",
        &[4u8; 100],
        dkrt_abi::storage::HASH_NONE,
        [0; 32],
    );
    f.storage.add_dir(SAMPLES, b"KICKS/SUB");
}

#[test]
fn a_store_route_device_gives_the_hash_of_the_table_entry() {
    let mut f = FakeCore::new();
    f.device = dkrt_abi::profiles::DN2.id;
    sample_files(&f.host);
    let mut head = [0u8; 64];
    let id = unsafe { probe(&f, b"samples/KICKS/K1", Some(&mut head)) }.unwrap();
    assert_eq!(
        id,
        FileId {
            root: SAMPLES,
            size: 100,
            mtime: 0,
            flags: ID_HASH_VALID,
            hash_kind: 2,
            hash: H
        }
    );
    assert_eq!(&head[..4], [3, 3, 3, 3]);
    assert_eq!(
        unsafe { probe(&f, b"samples/KICKS/none", None) },
        Err(E_NOT_FOUND)
    );
    assert_eq!(unsafe { probe(&f, b"samples/", None) }, Err(E_SEL_PATH));
    assert_eq!(
        unsafe { probe(&f, b"samples/KICKS/SUB", None) },
        Err(E_NOT_FOUND),
        "a folder is no sample (it does not open as a file)"
    );
}

#[test]
fn a_firmware_fs_device_has_a_hash_only_when_the_storage_gives_one() {
    let mut f = FakeCore::new();
    f.device = dkrt_abi::profiles::DT2.id;
    sample_files(&f.host);
    let hashed = unsafe { probe(&f, b"/KICKS/K1", None) }.unwrap();
    assert_eq!(
        hashed,
        FileId {
            root: SAMPLES,
            size: 100,
            mtime: 0,
            flags: ID_HASH_VALID,
            hash_kind: 2,
            hash: H
        }
    );
    let plain = unsafe { probe(&f, b"/KICKS/K2", None) }.unwrap();
    assert_eq!(
        plain,
        FileId {
            root: SAMPLES,
            size: 100,
            mtime: 0,
            flags: 0,
            hash_kind: 0,
            hash: [0; 32]
        }
    );
    assert!(!plain.same(&FileId {
        hash_kind: 2,
        flags: ID_HASH_VALID,
        hash: H,
        ..plain
    }));
    assert_eq!(unsafe { probe(&f, b"/", None) }, Err(E_SEL_PATH));
}

/// The route table (`dkrt_test_support::routes`) is the one answer to "who serves the samples root on this device"; the service's root and the key it
/// builds must agree with it for every read row of the samples root.
#[test]
fn the_sample_root_agrees_with_the_route_table() {
    use dkrt_test_support::routes::{Backend, Op, ROUTES};
    let mut rows = 0;
    for r in ROUTES
        .iter()
        .filter(|r| r.op == Op::Read && r.root == dkrt_abi::storage::ROOT_SAMPLES)
    {
        rows += 1;
        let mut host = FakeCore::new();
        host.device = r.profile.id;
        let root = root_of(&host);
        let mut out = [0u8; dkrt_sdk::sample_tracks::PATH];
        let n = dkrt_sdk::sample_tracks::rooted(root, r.path.as_bytes(), &mut out).unwrap();
        let backend = if root == ROOT_DN2 {
            Backend::Store
        } else {
            Backend::Fs
        };
        assert_eq!((backend, &out[..n]), (r.backend, r.key.as_bytes()), "{r:?}");
    }
    assert!(rows >= 4);
}
