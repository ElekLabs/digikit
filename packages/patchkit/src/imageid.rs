//! The image id: 8 bytes that identify the exact image a patch made, written into the runtime at patch time and reported by its INFO reply.
//!
//! Definition: the first 8 bytes of SHA-256 over `"DKIMG1"`, the stock base's SHA-256 (32 bytes), the decoded MAIN OS section
//! (`u32be` length, bytes) and the decoded DSP section (`u32be` length, bytes), with the 8 id bytes in MAIN OS set to zero.
//! The other sections of a patched image are the stock base's (preflight proves it), so these inputs fix the whole container: the
//! container is a deterministic function of the sections. The hash cannot cover the packed container, because repacking changes it
//! with the id in it; the decoded sections have a fixed layout, so the id is stamped after the hash is taken.
//!
//! The 8 bytes follow the marker `"DKIMGID1"` in the runtime's device object (`dkrt_image_id` in `core/runtime/src/cf/<dev>/device.s`).
//! The marker lets a reader find the field in a finished image and check the id against the sections without the patch inputs.

use crate::error::*;
use sha2::{Digest, Sha256};

pub const MARKER: &[u8; 8] = b"DKIMGID1";
/// The linker symbol of the id field (after the marker).
pub const SYMBOL: &str = "dkrt_image_id";

pub fn compute(base_sha256: &[u8], main_zeroed: &[u8], dsp: &[u8]) -> [u8; 8] {
    let mut h = Sha256::new();
    h.update(b"DKIMG1");
    h.update(base_sha256);
    h.update((main_zeroed.len() as u32).to_be_bytes());
    h.update(main_zeroed);
    h.update((dsp.len() as u32).to_be_bytes());
    h.update(dsp);
    h.finalize()[..8].try_into().unwrap()
}

/// The offset of the id field in a decoded MAIN OS section: the one marker's end. `None` when there is no marker (a stock image,
/// or a patch without the runtime); an error when there are several.
pub fn locate(main: &[u8]) -> Result<Option<usize>> {
    let mut found = None;
    for (i, w) in main.windows(MARKER.len()).enumerate() {
        if w == MARKER {
            if found.is_some() {
                return Err(err!("the image has more than one image id marker"));
            }
            found = Some(i + MARKER.len());
        }
    }
    match found {
        Some(at) if at + 8 <= main.len() => Ok(Some(at)),
        Some(_) => Err(err!("the image id marker is at the end of MAIN OS")),
        None => Ok(None),
    }
}

/// The id stamped into a finished image and whether it matches the sections: `(id, matches)`, or `None` without a runtime.
/// `base_sha256` is the stock base's hash (the patch record names it).
pub fn read(base_sha256: &[u8], main: &[u8], dsp: &[u8]) -> Result<Option<([u8; 8], bool)>> {
    let Some(at) = locate(main)? else {
        return Ok(None);
    };
    let id: [u8; 8] = main[at..at + 8].try_into().unwrap();
    let mut z = main.to_vec();
    z[at..at + 8].fill(0);
    Ok(Some((id, compute(base_sha256, &z, dsp) == id)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn main_with_id(id: [u8; 8]) -> Vec<u8> {
        let mut m = vec![7u8; 64];
        m[16..24].copy_from_slice(MARKER);
        m[24..32].copy_from_slice(&id);
        m
    }

    #[test]
    fn a_stamped_image_reads_back_and_matches() {
        let base = [3u8; 32];
        let dsp = vec![1u8; 40];
        let id = compute(&base, &main_with_id([0; 8]), &dsp);
        let m = main_with_id(id);
        assert_eq!(read(&base, &m, &dsp).unwrap(), Some((id, true)));
    }

    #[test]
    fn a_changed_byte_or_base_breaks_the_match() {
        let base = [3u8; 32];
        let dsp = vec![1u8; 40];
        let id = compute(&base, &main_with_id([0; 8]), &dsp);
        let mut m = main_with_id(id);
        m[50] ^= 1;
        assert!(!read(&base, &m, &dsp).unwrap().unwrap().1);
        assert!(
            !read(&[4u8; 32], &main_with_id(id), &dsp)
                .unwrap()
                .unwrap()
                .1
        );
        let mut d2 = dsp.clone();
        d2[0] ^= 1;
        assert!(!read(&base, &main_with_id(id), &d2).unwrap().unwrap().1);
    }

    #[test]
    fn no_marker_is_no_id_and_two_are_an_error() {
        assert_eq!(read(&[0; 32], &[0u8; 64], &[]).unwrap(), None);
        let mut m = main_with_id([0; 8]);
        m.extend_from_slice(MARKER);
        m.extend_from_slice(&[0; 8]);
        assert!(locate(&m).is_err());
    }
}
