//! The one decision of the storage layer: which backend serves a root on this device, and the path as that backend takes it. Backends are chosen by
//! device profile data (`samples_in_store`, whether the +Drive write adapter is in the image), never by a device id.
//!
//! - `ROOT_STORE` is always the store (modules, services, settings).
//! - `ROOT_SAMPLES` is the store under the `samples/` folder on a device that keeps its samples there (DN2); on a device with the firmware file system (DT2)
//!   it is read through that file system (`Backend::Fs`) and written through the +Drive adapter (`Backend::PlusDrive`).
//! - Any other root does not exist: `E_FORMAT` for a read, `E_CONTRACT` for a write (the codes the two front ends gave before they shared this file).
//!
//! The route table `dkrt_test_support::routes` is the data that `the_routes_agree_with_the_route_table` (here) holds this file to; the samples service
//! spells the same fact in its select records (`root_of`) and has the same test.
use dkrt_abi::platform::{Platform, StagedWriter};
use dkrt_abi::storage::{ROOT_SAMPLES, ROOT_STORE};
use dkrt_sdk::store::{PATH_BYTES, PATH_MAX};
use dkrt_sdk::{E_CONTRACT, E_FORMAT, E_SEL_PATH, E_TOO_BIG};

/// The device facts the routes need (`DeviceProfile::samples_in_store` and `sample_path_max`).
#[derive(Clone, Copy)]
pub(crate) struct Facts {
    pub in_store: bool,
    pub sample_path_max: u32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Backend {
    Store,
    Fs,
    PlusDrive,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Op {
    Read,
    Write,
}

/// Who serves `root` for `op`. `staged`: the +Drive write adapter is in the image.
pub(crate) fn decide(root: u32, in_store: bool, staged: bool, op: Op) -> Result<Backend, i32> {
    let refused = if op == Op::Read { E_FORMAT } else { E_CONTRACT };
    if root == ROOT_STORE {
        return Ok(Backend::Store);
    }
    if root != ROOT_SAMPLES {
        return Err(refused);
    }
    if in_store {
        return Ok(Backend::Store);
    }
    if op == Op::Read {
        return Ok(Backend::Fs);
    }
    if staged {
        Ok(Backend::PlusDrive)
    } else {
        Err(refused) // no adapter in the image: the root does not exist for a write
    }
}

/// The backend of a read (`Storage`).
pub(crate) fn route(root: u32, f: &Facts) -> Result<Backend, i32> {
    decide(root, f.in_store, false, Op::Read)
}

/// The backend of a write (the `file_*` calls), and the prefix that the store path takes: `samples/` for the samples root of a device with them in the store.
pub(crate) fn root_route<P: Platform>(p: &P, root: u32) -> Result<(&'static [u8], bool), i32> {
    match decide(root, p.samples_in_store(), StagedWriter::present(p), Op::Write)? {
        Backend::Store if root == ROOT_SAMPLES => Ok((b"samples/", false)),
        Backend::Store => Ok((b"", false)),
        _ => Ok((b"", true)),
    }
}

/// Longest root-relative path of `root` (0 for another root).
pub(crate) fn limit(root: u32, f: &Facts) -> usize {
    if root == ROOT_STORE {
        return PATH_MAX;
    }
    if root == ROOT_SAMPLES {
        return f.sample_path_max as usize;
    }
    0
}

/// The route of `root`, after the length check of `path` (the same `E_TOO_BIG` as the listing request).
pub(crate) fn prepare(root: u32, f: &Facts, path: &[u8]) -> Result<Backend, i32> {
    let r = route(root, f)?;
    if path.len() > limit(root, f) {
        return Err(E_TOO_BIG);
    }
    Ok(r)
}

/// The store path of a root-relative `path`: `samples/` in front for `ROOT_SAMPLES`, NUL-terminated in `out`. Returns the length without the NUL.
pub(crate) fn store_key(root: u32, path: &[u8], out: &mut [u8; PATH_BYTES]) -> Result<usize, i32> {
    if path.contains(&0) {
        return Err(E_SEL_PATH);
    }
    let prefix: &[u8] = if root == ROOT_SAMPLES { b"samples" } else { b"" };
    let sep = if !prefix.is_empty() && !path.is_empty() { 1 } else { 0 };
    let total = prefix.len() + sep + path.len();
    if total > PATH_MAX {
        return Err(E_TOO_BIG);
    }
    *out = [0; PATH_BYTES];
    out[..prefix.len()].copy_from_slice(prefix);
    if sep == 1 {
        out[prefix.len()] = b'/';
    }
    out[prefix.len() + sep..total].copy_from_slice(path);
    Ok(total)
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake_platform::FakePlatform;

    const IN_STORE: Facts = Facts { in_store: true, sample_path_max: 150 };
    const ON_FS: Facts = Facts { in_store: false, sample_path_max: 255 };

    #[test]
    fn store_key_prefixes_samples() {
        let mut z = [0xffu8; PATH_BYTES];
        let n = store_key(ROOT_SAMPLES, b"KICKS/K1", &mut z).unwrap();
        assert_eq!(&z[..n], b"samples/KICKS/K1");
        assert_eq!(z[n], 0);
        let n = store_key(ROOT_SAMPLES, b"", &mut z).unwrap();
        assert_eq!(&z[..n], b"samples");
        let n = store_key(ROOT_STORE, b"modules/a.dkm", &mut z).unwrap();
        assert_eq!(&z[..n], b"modules/a.dkm");
    }

    #[test]
    fn store_key_limits_and_nul() {
        let mut z = [0u8; PATH_BYTES];
        assert_eq!(store_key(ROOT_SAMPLES, &[b'a'; 151], &mut z), Ok(159));
        assert_eq!(store_key(ROOT_SAMPLES, &[b'a'; 152], &mut z), Err(E_TOO_BIG));
        assert_eq!(store_key(ROOT_STORE, &[b'a'; 159], &mut z), Ok(159));
        assert_eq!(store_key(ROOT_STORE, &[b'a'; 160], &mut z), Err(E_TOO_BIG));
        assert_eq!(store_key(ROOT_STORE, b"a\0b", &mut z), Err(E_SEL_PATH));
    }

    #[test]
    fn routes_follow_the_profile_facts() {
        assert_eq!(route(ROOT_STORE, &IN_STORE), Ok(Backend::Store));
        assert_eq!(route(ROOT_STORE, &ON_FS), Ok(Backend::Store));
        assert_eq!(route(ROOT_SAMPLES, &IN_STORE), Ok(Backend::Store));
        assert_eq!(route(ROOT_SAMPLES, &ON_FS), Ok(Backend::Fs));
        assert_eq!(route(99, &ON_FS), Err(E_FORMAT));
    }

    #[test]
    fn path_limits_per_root() {
        assert_eq!(limit(ROOT_STORE, &IN_STORE), 159);
        assert_eq!(limit(ROOT_SAMPLES, &IN_STORE), 150);
        assert_eq!(limit(99, &IN_STORE), 0);
        assert_eq!(prepare(ROOT_SAMPLES, &IN_STORE, &[b'a'; 150]), Ok(Backend::Store));
        assert_eq!(prepare(ROOT_SAMPLES, &IN_STORE, &[b'a'; 151]), Err(E_TOO_BIG));
        assert_eq!(prepare(ROOT_STORE, &IN_STORE, &[b'a'; 160]), Err(E_TOO_BIG));
    }

    /// The route table (`dkrt_test_support::routes`) is the one answer to "who serves this root on this device".
    #[test]
    fn the_routes_agree_with_the_route_table() {
        use dkrt_test_support::routes::{Backend as B, Op as TableOp, ROUTES};
        let mut rows = 0;
        for r in ROUTES.iter() {
            rows += 1;
            let f = FakePlatform::new(r.profile);
            f.staged.set(r.staged);
            let (backend, key) = if r.op == TableOp::Read {
                let facts = Facts { in_store: r.profile.samples_in_store, sample_path_max: r.profile.sample_path_max };
                match route(r.root, &facts) {
                    Ok(Backend::Store) => {
                        let mut z = [0u8; PATH_BYTES];
                        let n = store_key(r.root, r.path.as_bytes(), &mut z).unwrap();
                        (B::Store, std::string::String::from_utf8(z[..n].to_vec()).unwrap())
                    }
                    Ok(Backend::Fs) => (B::Fs, std::format!("/{}", r.path)),
                    Ok(Backend::PlusDrive) => unreachable!("a read is never +Drive"),
                    Err(_) => (B::Refused, std::string::String::new()),
                }
            } else {
                match root_route(&f, r.root) {
                    Ok((prefix, false)) => (B::Store, std::format!("{}{}", std::string::String::from_utf8_lossy(prefix), r.path)),
                    Ok((_, true)) => (B::PlusDrive, r.path.into()),
                    Err(_) => (B::Refused, std::string::String::new()),
                }
            };
            assert_eq!((backend, key.as_str()), (r.backend, r.key), "{r:?}");
        }
        assert!(rows >= 20);
    }
}
