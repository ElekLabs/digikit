//! Files and paths of the fake store.
use dkrt_svc_project::dkp::{self, Image};
use dkrt_svc_project::name;

pub(crate) const DEV: u16 = 0x1234;
pub(crate) const OWNER: &[u8] = b"test/owner";

pub(crate) fn n16(s: &[u8]) -> [u8; 16] {
    let mut n = [0u8; 16];
    n[..s.len()].copy_from_slice(s);
    n
}

/// A project file as a store holds it: the image bytes up to `used`.
pub(crate) fn file_bytes(
    slot: u8,
    name: &[u8],
    stamp: u32,
    counter: u32,
    data: &[(&[u8], &[u8])],
) -> Vec<u8> {
    let mut i = Box::new(Image::new());
    dkp::reset(&mut i.0, DEV, slot, &n16(name), stamp);
    for (k, v) in data {
        let bare = k.strip_prefix(b"test/owner/").unwrap_or(k);
        dkp::put(&mut i.0, OWNER, bare, v).unwrap();
    }
    dkp::seal(&mut i.0, counter);
    let used = dkp::head_of(&i.0).used as usize;
    i.0[..used].to_vec()
}

pub(crate) fn path(slot: u32, name: &[u8]) -> Vec<u8> {
    name::file_path(slot, &n16(name)).as_bytes().to_vec()
}
